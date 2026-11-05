//! Project session observations into Slint models.
use super::*;
use slint::{ModelRc, VecModel};
fn update_model<T: Clone + PartialEq + 'static>(current: ModelRc<T>, values: Vec<T>) -> ModelRc<T> {
    let Some(rows) = current.as_any().downcast_ref::<VecModel<T>>() else {
        return Rc::new(VecModel::from(values)).into();
    };
    for (index, value) in values.iter().take(rows.row_count()).enumerate() {
        if rows.row_data(index).as_ref() != Some(value) {
            rows.set_row_data(index, value.clone());
        }
    }
    while rows.row_count() > values.len() {
        rows.remove(values.len());
    }
    for value in values.into_iter().skip(rows.row_count()) {
        rows.push(value);
    }
    current
}
impl DesktopController {
    pub(super) fn refresh(&self) {
        let s = self.session.borrow();
        let main_open_pending = self
            .open_jobs
            .borrow()
            .iter()
            .any(|job| job.target == OpenTarget::Main);
        self.ui.set_native_open_pending(main_open_pending);
        platform::set_busy(s.needs_quit_confirmation());
        self.ui.set_closing(s.is_closing());
        if s.is_closing() {
            self.export_draft.borrow_mut().take();
            let _ = self.export_ui.hide();
        }
        self.ui.set_check_only(s.state.mode == RunMode::DryRun);
        self.ui.set_selection(update_model(
            self.ui.get_selection(),
            s.state
                .selection
                .iter()
                .map(|path| view_model::path(&eml_protocol::TerminalPath::new(path).to_string()))
                .collect(),
        ));
        self.ui
            .set_reviewing(s.state.reviewing || s.entries.is_empty());
        self.ui.set_export_running(s.export.is_some());
        self.ui.set_stop_requested(
            s.active
                .as_ref()
                .is_some_and(|a| a.stop_requested || a.force_requested),
        );
        self.ui.set_selected_item(s.state.selected_item as i32);
        self.ui.set_selected_run(s.state.selected as i32);
        self.ui.set_has_reports(!s.entries.is_empty());
        self.ui.set_worker_running(s.active.is_some());
        self.ui.set_start_label(if s.memory_full() {
            "Waiting for space — close a finished run".into()
        } else if s.state.queue.capacity_error().is_some() {
            "Close a finished run to add more".into()
        } else if s.active.is_some() || s.report_task.is_some() {
            "Queue request".into()
        } else if s.state.mode == RunMode::DryRun {
            "Start check".into()
        } else {
            "Create copies".into()
        });
        self.ui
            .set_can_start(!s.state.selection.is_empty() && s.admission_error().is_none());
        self.ui.set_start_reason(
            s.admission_error()
                .map_or_else(
                    || {
                        format!(
                            "{} using the reviewed paths and current destination",
                            self.ui.get_start_label()
                        )
                    },
                    |error| error.to_string(),
                )
                .into(),
        );
        let activity = s
            .active
            .as_ref()
            .and_then(|active| s.entries.iter().find(|e| e.id == active.id))
            .map(|entry| {
                format!(
                    "Run {} · {}",
                    entry.id,
                    status(entry, s.active.as_ref()).label()
                )
            })
            .unwrap_or_else(|| {
                s.export.as_ref().map_or_else(String::new, |job| {
                    if s.quit_when_idle || s.force_quit {
                        format!(
                            "Finishing report export for Run {} before quitting",
                            job.run
                        )
                    } else {
                        format!("Saving report for Run {}", job.run)
                    }
                })
            });
        self.ui.set_activity_label(activity.into());
        self.ui
            .set_settings_warning(s.preferences.warning().unwrap_or_default().into());
        self.ui.set_queue_notice(
            if s.entries
                .iter()
                .filter(|entry| entry.completion.is_some() && entry.stored.is_none())
                .count()
                >= 3
            {
                "Waiting for space — close a finished run".into()
            } else {
                "".into()
            },
        );
        self.ui
            .set_destination_beside(s.preferences.destination().is_none());
        self.ui.set_destination_label(
            s.preferences
                .destination()
                .map(|p| eml_protocol::TerminalPath::new(p).to_string())
                .unwrap_or_else(|| "Beside originals".into())
                .into(),
        );
        self.ui.set_runs(update_model(
            self.ui.get_runs(),
            s.entries
                .iter()
                .map(|entry| {
                    let mut view = run_view(
                        entry.id,
                        &entry.request,
                        status(entry, s.active.as_ref()).label(),
                    );
                    view.unseen = entry.presentation.unseen;
                    view
                })
                .collect(),
        ));
        self.ui.set_waiting(update_model(
            self.ui.get_waiting(),
            s.state
                .queue
                .waiting_requests()
                .map(|(id, request)| run_view(id, request, RunStatus::Queued.label()))
                .collect(),
        ));
        let Some(entry) = s.entries.get(s.state.selected) else {
            self.clear_report();
            return;
        };
        self.ui.set_report_loading(
            entry.stored.is_some() && entry.report.is_none() && entry.load_error.is_none(),
        );
        self.ui.set_report_load_notice(entry.load_error.as_ref().map_or_else(
            || "Loading report…".into(), |error| format!("Report unavailable: {error}. Close this result to release its session storage.")).into());
        self.ui.set_can_reveal_report(entry.saved_report.is_some());
        self.ui.set_saved_report_path(
            entry
                .saved_report
                .as_ref()
                .map(|path| view_model::path(&eml_protocol::TerminalPath::new(path).to_string()))
                .unwrap_or_default(),
        );
        self.ui.set_run_label(format!("Run {}", entry.id).into());
        self.ui.set_run_context(
            format!(
                "{} · {}",
                vocabulary::mode_label(entry.request.request().mode),
                request_destination(&entry.request)
            )
            .into(),
        );
        let run_status = status(entry, s.active.as_ref());
        self.ui.set_run_status(run_status.label().into());
        self.ui.set_run_attention(matches!(
            run_status,
            RunStatus::CompletedWithFailures
                | RunStatus::StartFailed
                | RunStatus::IdentityFailure
                | RunStatus::ForceStopped
                | RunStatus::RequestRefused
                | RunStatus::Incomplete
        ));
        self.ui.set_report_running(
            s.active
                .as_ref()
                .is_some_and(|active| active.id == entry.id),
        );
        self.ui.set_can_export(
            entry
                .completion
                .as_ref()
                .is_some_and(|c| !c.jsonl.is_empty())
                && s.export.is_none()
                && self.export_draft.borrow().is_none(),
        );
        self.ui.set_can_dismiss(
            entry.completion.is_some() || entry.stored.is_some() || entry.launch_error.is_some(),
        );
        self.ui.set_can_create(can_create(entry));
        self.ui.set_can_review(
            entry.completion.is_some() || entry.stored.is_some() || entry.launch_error.is_some(),
        );
        self.ui.set_review_label(
            if can_create(entry) {
                "Review for copy creation"
            } else {
                "Review inputs again"
            }
            .into(),
        );
        self.ui.set_report_details(
            entry
                .completion
                .as_ref()
                .map(|c| {
                    eml_protocol::DisplayText::new(&String::from_utf8_lossy(&c.stderr)).to_string()
                })
                .unwrap_or_default()
                .into(),
        );
        let mut diagnostics = entry
            .completion
            .as_ref()
            .map(view_model::completion_diagnostics)
            .unwrap_or_default();
        for error in [&entry.launch_error, &entry.worker_error]
            .into_iter()
            .flatten()
        {
            diagnostics.push(DetailView { text: error.into() });
        }
        if let Some(error) = &entry.load_error {
            diagnostics.push(DetailView {
                text: format!(
                    "Report unavailable: {error}. Close this result to release its session storage."
                )
                .into(),
            });
        }
        if let Some(error) = &entry.storage_error {
            diagnostics.push(DetailView {
                text: format!(
                    "This report is kept in memory because temporary storage failed: {error}"
                )
                .into(),
            });
        }
        if let Some(handle) = &entry.report {
            handle.with_report(|report| diagnostics.extend(view_model::report_diagnostics(report)));
        }
        self.ui.set_report_diagnostics(update_model(
            self.ui.get_report_diagnostics(),
            diagnostics
                .into_iter()
                .map(|row| DetailView {
                    text: eml_protocol::DisplayText::new(row.text.as_str())
                        .to_string()
                        .into(),
                })
                .collect(),
        ));
        let previous = *self.last_view.borrow();
        if let Some(previous) = previous
            && (previous.run != entry.id || previous.item != s.state.selected_item)
        {
            self.ui.set_details_scroll_y(0.0);
            self.ui.set_details_expanded(false);
            if previous.run != entry.id {
                self.clear_items();
            }
        }
        let Some(handle) = &entry.report else {
            self.clear_items();
            return;
        };
        handle.with_report(|report| {
            self.ui
                .set_can_reveal(report.reveal_path(s.state.selected_item).is_some());
            let revision = PresentationRevision {
                run: entry.id,
                item: s.state.selected_item,
                attention_only: self.ui.get_attention_only(),
                started: report.start().is_some(),
                items: report.items().len(),
                summary: report.summary().is_some(),
                failure: report.failure(),
            };
            if *self.last_view.borrow() == Some(revision) {
                return;
            }
            *self.last_view.borrow_mut() = Some(revision);
            self.ui
                .set_work_total(report.start().map_or(0, |s| s.total as i32));
            self.ui.set_work_completed(report.items().len() as i32);

            self.ui.set_items(update_model(
                self.ui.get_items(),
                report
                    .items()
                    .iter()
                    .enumerate()
                    .filter(|(_, item)| {
                        !self.ui.get_attention_only() || view_model::item(item).attention
                    })
                    .map(|(index, item)| {
                        let mut row = view_model::item(item);
                        row.original_index = index as i32;
                        row
                    })
                    .collect(),
            ));
            self.ui.set_selected_visible_row(
                self.ui
                    .get_items()
                    .iter()
                    .position(|row| row.original_index == s.state.selected_item as i32)
                    .map_or(-1, |index| index as i32),
            );
            self.ui.set_counts(update_model(
                self.ui.get_counts(),
                view_model::counts(report),
            ));
            let Some(item) = report.items().get(s.state.selected_item) else {
                self.ui.set_can_copy_source(false);
                self.ui.set_can_copy_destination(false);
                self.ui.set_has_item(false);
                return;
            };
            let view = view_model::item(item);
            self.ui.set_has_item(true);
            self.ui.set_can_copy_source(
                report
                    .source_path(s.state.selected_item)
                    .and_then(|p| p.into_os_string().into_string().ok())
                    .is_some(),
            );
            self.ui.set_can_copy_destination(
                report
                    .destination_path(s.state.selected_item)
                    .and_then(|p| p.into_os_string().into_string().ok())
                    .is_some(),
            );
            self.ui.set_item_source(view.source);
            self.ui.set_item_source_path(view.path);
            self.ui
                .set_item_destination_path(view_model::path(view.destination.as_str()));
            self.ui.set_item_destination(view.destination);
            self.ui.set_item_status(view.status);
            self.ui.set_item_attention(view.attention);
            let (details, parts, fields) = view_model::details(item);
            self.ui
                .set_details(update_model(self.ui.get_details(), details));
            self.ui.set_byte_counts(update_model(
                self.ui.get_byte_counts(),
                view_model::byte_counts(item),
            ));
            self.ui.set_removal_heading(parts.into());
            self.ui.set_field_heading(fields.into());
            self.ui.set_removals(update_model(
                self.ui.get_removals(),
                view_model::removals(item),
            ));
            self.ui.set_stripped(update_model(
                self.ui.get_stripped(),
                item.stripped_fields
                    .iter()
                    .map(|name| DetailView {
                        text: name.clone().into(),
                    })
                    .collect(),
            ));
        });
    }
    fn clear_items(&self) {
        self.ui.set_keyboard_navigation(false);
        self.ui.set_messages_scroll_y(0.0);
        self.ui.set_attention_only(false);
        self.ui.set_selected_visible_row(-1);
        self.ui.set_work_total(0);
        self.ui.set_work_completed(0);
        self.ui
            .set_items(update_model(self.ui.get_items(), Vec::new()));
        self.ui
            .set_counts(update_model(self.ui.get_counts(), Vec::new()));
        self.ui.set_can_copy_source(false);
        self.ui.set_can_copy_destination(false);
        self.ui.set_has_item(false);
        self.ui.set_item_attention(false);
        self.ui
            .set_details(update_model(self.ui.get_details(), Vec::new()));
        self.ui
            .set_removals(update_model(self.ui.get_removals(), Vec::new()));
        self.ui
            .set_stripped(update_model(self.ui.get_stripped(), Vec::new()));
        self.ui
            .set_byte_counts(update_model(self.ui.get_byte_counts(), Vec::new()));
        *self.last_view.borrow_mut() = None;
        self.ui.set_can_reveal(false);
    }
    fn clear_report(&self) {
        self.ui.set_can_reveal_report(false);
        self.ui.set_saved_report_path(PathView::default());
        self.ui.set_report_loading(false);
        self.ui.set_report_details("".into());
        self.ui
            .set_report_diagnostics(update_model(self.ui.get_report_diagnostics(), Vec::new()));
        self.clear_items();
        self.ui
            .set_run_label("Drop messages or choose files to begin".into());
        self.ui.set_run_status("".into());
        self.ui.set_run_attention(false);
        self.ui.set_run_context("".into());
        self.ui.set_report_running(false);
        self.ui.set_can_export(false);
        self.ui.set_can_dismiss(false);
        self.ui.set_can_create(false);
        self.ui.set_can_review(false);
    }
}
