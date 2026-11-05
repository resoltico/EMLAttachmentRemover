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
        self.ui
            .set_can_edit(!s.is_closing() && s.state.confirmation.is_none());
        self.ui.set_session_started(s.state.has_started);
        self.ui.set_starts_after(
            s.active
                .as_ref()
                .map_or_else(String::new, |active| {
                    format!("Starts after Run {}", active.id)
                })
                .into(),
        );
        self.ui.set_recent_destinations(update_model(
            self.ui.get_recent_destinations(),
            s.preferences
                .recent_destinations()
                .iter()
                .map(|path| {
                    path.strip_prefix(&self.home)
                        .map_or_else(
                            |_| eml_protocol::TerminalPath::new(path).to_string(),
                            |relative| format!("~/{}", eml_protocol::TerminalPath::new(relative)),
                        )
                        .into()
                })
                .collect(),
        ));
        if s.is_closing() {
            self.pending_save.borrow_mut().take();
        }
        self.ui.set_check_only(s.state.mode == RunMode::DryRun);
        self.ui.set_selection(update_model(
            self.ui.get_selection(),
            s.state
                .selection
                .iter()
                .map(|path| {
                    let mut display =
                        view_model::path(&eml_protocol::TerminalPath::new(path).to_string());
                    if let Some(parent) = path.parent()
                        && let Ok(relative) = parent.strip_prefix(&self.home)
                    {
                        display.directory =
                            format!("~/{}", eml_protocol::TerminalPath::new(relative)).into();
                    }
                    SelectionView {
                        path: display,
                        folder: path.is_dir(),
                    }
                })
                .collect(),
        ));
        self.ui.set_reviewing(s.state.view == state::View::Draft);
        self.ui
            .set_waiting_view(matches!(s.state.view, state::View::Waiting(_)));
        self.ui.set_export_running(s.export.is_some());
        self.ui.set_stop_requested(
            s.active
                .as_ref()
                .is_some_and(|a| a.stop != state::Stop::Running),
        );
        let selected = s.selected_index().map(|index| index as i32).or_else(|| {
            if let state::View::Waiting(id) = s.state.view {
                s.state
                    .queue
                    .waiting_requests()
                    .position(|(run, _)| run == id)
                    .map(|index| (s.entries.len() + index) as i32)
            } else {
                None
            }
        });
        self.ui.set_selected_run(selected.unwrap_or(-1));
        self.ui.set_unseen_count(
            s.entries
                .iter()
                .filter(|entry| entry.presentation.unseen)
                .count() as i32,
        );
        self.ui.set_can_stop(self.can_stop());
        self.ui.set_can_force(self.can_force());
        self.ui.set_can_filter(self.can_filter());
        self.ui.set_can_sort(self.can_sort());
        self.ui.set_can_details(self.can_details());
        self.ui
            .set_reveal_output_label(platform::file_manager_label().into());
        self.ui.set_can_show_copy(self.can_show_copy());
        self.ui.set_can_reveal_original(self.can_show_original());
        self.ui.set_can_copy_source(self.can_copy_path(false));
        self.ui.set_can_copy_destination(self.can_copy_path(true));
        self.ui.set_start_menu_label(
            if s.active.is_some() {
                "Add to Queue"
            } else if s.state.mode == RunMode::DryRun {
                "Start Check"
            } else {
                "Create Copies"
            }
            .into(),
        );
        self.update_text_actions();
        self.present_feedback();
        self.ui.set_has_reports(!s.entries.is_empty());
        self.ui.set_worker_running(s.active.is_some());
        self.ui.set_start_label(if s.memory_full() {
            "Waiting for space — close a finished run".into()
        } else if matches!(s.state.queue.capacity_error(), Some(QueueError::Full)) {
            "Queue full — remove a waiting run".into()
        } else if s.state.queue.capacity_error().is_some() {
            "Close a finished run to add more".into()
        } else if s.active.is_some() || s.report_task.is_some() {
            "Add to Queue".into()
        } else if s.state.mode == RunMode::DryRun {
            "Start Check".into()
        } else {
            "Create Copies".into()
        });
        self.ui.set_can_start(self.can_start());
        self.ui.set_start_reason(
            s.admission_error()
                .map_or_else(
                    || {
                        format!(
                            "{} using the reviewed paths and current destination",
                            self.ui.get_start_label()
                        )
                    },
                    |error| wording::queue_error(&error),
                )
                .into(),
        );
        self.ui
            .set_settings_warning(s.preferences.warning().unwrap_or_default().into());
        self.ui.set_queue_notice(if s.memory_full() {
            "Waiting for space — close a finished run".into()
        } else {
            "".into()
        });
        self.ui
            .set_destination_beside(s.preferences.destination().is_none());
        self.ui.set_destination_label(
            s.preferences
                .destination()
                .map(|path| folder_label(path, &self.home))
                .unwrap_or_else(|| "Next to originals".into())
                .into(),
        );
        let mut run_views: Vec<_> = s
            .entries
            .iter()
            .map(|entry| {
                let mut view = run_view(entry.id, &entry.request, status(entry, s.active.as_ref()));
                view.unseen = entry.presentation.unseen;
                view.closable = self.can_close_run(&s, entry);
                view.status = wording::sidebar(
                    status(entry, s.active.as_ref()),
                    entry.request.request().mode,
                    entry.presentation.tally,
                    entry.presentation.total,
                )
                .into();
                if entry.storage_error.is_some() {
                    view.status = format!("Kept in memory · {}", view.status).into();
                    view.tone = Tone::Attention;
                }
                view
            })
            .collect();
        run_views.extend(
            s.state
                .queue
                .waiting_requests()
                .map(|(id, request)| run_view(id, request, RunStatus::Queued)),
        );
        self.ui
            .set_runs(update_model(self.ui.get_runs(), run_views));
        self.ui.set_waiting_count(s.state.queue.waiting() as i32);
        if let state::View::Waiting(id) = s.state.view {
            self.clear_report();
            if let Some((_, request)) = s.state.queue.waiting_requests().find(|(run, _)| *run == id)
            {
                self.ui.set_waiting_token(id.to_string().into());
                self.ui
                    .set_waiting_title(format!("Run {id} · Waiting").into());
                self.ui
                    .set_waiting_context(request_destination(request).into());
            }
            return;
        }
        let Some(entry) = s.selected_entry() else {
            self.clear_report();
            return;
        };
        self.ui.set_report_loading(
            entry.stored.is_some()
                && entry.report.is_none()
                && entry.load_error.is_none()
                && !s.memory_full(),
        );
        self.ui
            .set_report_load_notice(if entry.report.is_none() && s.memory_full() {
                "Close a finished run to load this report".into()
            } else {
                "Loading report…".into()
            });
        self.ui.set_run_label(format!("Run {}", entry.id).into());
        let run_status = status(entry, s.active.as_ref());
        self.ui.set_run_status(wording::phase(run_status).into());
        self.ui.set_run_title(
            wording::run_title(
                entry.id,
                run_status,
                entry.request.request().mode,
                entry.presentation.tally,
                entry.presentation.total,
            )
            .into(),
        );
        let inputs = &entry.request.request().paths;
        let first = inputs
            .first()
            .and_then(|path| path.file_name())
            .map(|name| eml_protocol::TerminalPath::new(Path::new(name)).to_string())
            .unwrap_or_default();
        self.ui.set_run_context(
            format!(
                "{first}{} → {}",
                if inputs.len() > 1 {
                    format!(" +{}", inputs.len() - 1)
                } else {
                    String::new()
                },
                request_destination(&entry.request)
            )
            .into(),
        );
        let running = s
            .active
            .as_ref()
            .is_some_and(|active| active.id == entry.id);
        self.ui.set_report_running(running);
        self.ui.set_can_export(self.can_save_report());
        self.ui.set_can_dismiss(self.can_close_run(&s, entry));
        self.ui.set_is_check_result(can_create(entry));
        self.ui.set_can_create(self.can_convert_check());
        self.ui.set_can_review(self.can_review());
        self.ui.set_review_label(
            if matches!(
                run_status,
                RunStatus::Stopped | RunStatus::StoppedBeforeStart | RunStatus::ForceStopped
            ) {
                "Continue…"
            } else {
                "Run Again…"
            }
            .into(),
        );
        let roots = super::navigation::verified_roots(entry);
        self.ui.set_can_reveal_output(self.can_show_output());
        self.ui.set_output_roots(update_model(
            self.ui.get_output_roots(),
            roots
                .iter()
                .take(8)
                .map(|path| eml_protocol::TerminalPath::new(path).to_string().into())
                .collect(),
        ));
        self.ui
            .set_extra_output_roots(roots.len().saturating_sub(8) as i32);
        self.ui
            .set_reveal_output_label(platform::file_manager_label().into());
        self.ui.set_progress_label(
            entry
                .presentation
                .total
                .map_or_else(
                    || "Finding messages…".into(),
                    |total| format!("{} of {total} messages", entry.presentation.tally.processed),
                )
                .into(),
        );
        self.ui.set_estimate(
            if let Some(active) = s.active.as_ref().filter(|active| active.id == entry.id)
                && entry.presentation.total.is_some()
            {
                wording::estimate(
                    active
                        .estimate
                        .remaining(now_ms(), entry.presentation.total),
                )
                .into()
            } else {
                "".into()
            },
        );
        self.ui
            .set_work_total(entry.presentation.total.unwrap_or(0) as i32);
        self.ui
            .set_work_completed(entry.presentation.tally.processed as i32);
        self.ui.set_filter(entry.presentation.filter.index());
        self.ui
            .set_sort_column(match entry.presentation.sort.column {
                state::Column::Message => 0,
                state::Column::Folder => 1,
                state::Column::Outcome => 2,
            });
        self.ui
            .set_sort_descending(entry.presentation.sort.descending);
        self.ui.set_chips(update_model(
            self.ui.get_chips(),
            if entry.presentation.total.is_some() {
                chips(entry.presentation.tally, entry.presentation.filter)
            } else {
                Vec::new()
            },
        ));
        let mut diagnostics = entry
            .completion
            .as_ref()
            .map(view_model::completion_diagnostics)
            .unwrap_or_default();
        for error in [
            &entry.launch_error,
            &entry.worker_error,
            &entry.load_error,
            &entry.storage_error,
        ]
        .into_iter()
        .flatten()
        {
            diagnostics.push(DetailView { text: error.into() });
        }
        let mut notice = if entry.launch_error.is_some() {
            "Couldn’t start. Check the app installation and your folders, then run again."
                .to_owned()
        } else if entry.load_error.is_some() {
            "Couldn’t load this session report. Close it or run the request again.".to_owned()
        } else if entry.storage_error.is_some() {
            "This report stays in memory because session storage failed.".to_owned()
        } else if entry.report.is_none() && s.memory_full() {
            "Close a finished run to load this report.".to_owned()
        } else {
            String::new()
        };
        if let Some(handle) = &entry.report {
            handle.with_report(|report| {
                diagnostics.extend(view_model::report_diagnostics(report));
                if completion_state(entry).is_some_and(|(_,failure)|failure==Some(WorkerFailure::Identity)) {
                    notice="The app and its email processor don’t match. Install the app from one complete download.".into();
                } else if report.failure().is_some() || report.unreported_item_count().is_some_and(|count|count>0) {
                    notice="This report is incomplete. Check the destination before continuing; some results may be unknown.".into();
                }else if let Some(error)=report.summary().and_then(|summary|summary.run_error.value()) {
                    notice=if let Some(code)=eml_core::ErrorCode::from_wire_name(&error.code){let value=wording::failure(code);format!("{}. {}",value.headline,if code==eml_core::ErrorCode::NoInputs{"Add email files or choose another folder."}else{value.guidance})}else{"The run could not finish. See Details and save a report if you need help.".into()};
                    if report.start().is_some_and(|start|!start.collection_complete){notice=format!("The folder search did not finish. Counts cover only messages found so far. {notice}");}
                }
            });
        }
        self.ui.set_run_notice(notice.into());
        self.ui.set_report_details(
            entry
                .completion
                .as_ref()
                .map(|completion| {
                    eml_protocol::DisplayText::new(&String::from_utf8_lossy(&completion.stderr))
                        .to_string()
                })
                .unwrap_or_default()
                .into(),
        );
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
        if previous.is_some_and(|previous| previous.run != entry.id) {
            self.ui.set_messages_scroll_y(0.0);
            self.clear_items();
        }
        if previous.is_some_and(|previous| {
            previous.run != entry.id || previous.item != entry.presentation.selected
        }) {
            self.ui.set_details_scroll_y(0.0);
        }
        let Some(handle) = &entry.report else {
            self.clear_items();
            return;
        };
        handle.with_report(|report| {
            let selected = entry.presentation.selected;
            self.ui
                .set_selected_item(selected.map_or(-1, |index| index as i32));
            self.ui.set_can_reveal(
                selected
                    .and_then(|index| report.reveal_path(index))
                    .is_some(),
            );
            let revision = PresentationRevision {
                run: entry.id,
                item: selected,
                filter: entry.presentation.filter,
                sort: entry.presentation.sort,
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
                .set_folder_column_needed(folder_column_needed(report, &entry.request));
            self.ui.set_items(update_model(
                self.ui.get_items(),
                table_rows(
                    report,
                    &entry.request,
                    entry.presentation.filter,
                    entry.presentation.sort,
                ),
            ));
            self.ui.set_selected_visible_row(
                self.ui
                    .get_items()
                    .iter()
                    .position(|row| Some(row.original_index as usize) == selected)
                    .map_or(-1, |index| index as i32),
            );
            let Some(index) = selected else {
                self.clear_selection();
                return;
            };
            let Some(item) = report.items().get(index) else {
                self.clear_selection();
                return;
            };
            let view = view_model::item(item);
            self.ui.set_has_item(true);
            let mut source_view = view.path;
            if let Some(parent) = report.source_path(index).as_deref().and_then(Path::parent) {
                source_view.directory = folder_label(parent, &self.home).into();
            }
            self.ui.set_item_source_path(source_view);
            self.ui.set_item_destination_path(
                item.destination
                    .value()
                    .map(|path| view_model::path(&path.display))
                    .unwrap_or_default(),
            );
            let words = view_model::item_words(item);
            self.ui.set_item_headline(words.headline.into());
            self.ui.set_item_guidance(view_model::guidance(item).into());
            self.ui
                .set_item_policy_notice(view_model::policy_notice(item).into());
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
    pub(super) fn present_feedback(&self) {
        let feedback = self.feedback.borrow();
        let (label, text, details, tone) = match &*feedback {
            state::Feedback::None => (
                String::new(),
                String::new(),
                String::new(),
                state::FeedbackTone::Info,
            ),
            state::Feedback::Message {
                label,
                text,
                details,
                tone,
            } => (label.clone(), text.clone(), details.clone(), *tone),
            state::Feedback::SavedReport { run, path, .. } => (
                String::new(),
                format!(
                    "Report saved for Run {run}: {}",
                    eml_protocol::TerminalPath::new(
                        path.file_name().map(Path::new).unwrap_or(path)
                    )
                ),
                String::new(),
                state::FeedbackTone::Success,
            ),
        };
        self.ui.set_operation_label(label.into());
        self.ui
            .set_operation_feedback(eml_protocol::DisplayText::new(&text).to_string().into());
        self.ui
            .set_operation_details(eml_protocol::DisplayText::new(&details).to_string().into());
        self.ui.set_operation_tone(match tone {
            state::FeedbackTone::Info => Tone::Neutral,
            state::FeedbackTone::Success => Tone::Ok,
            state::FeedbackTone::Attention => Tone::Attention,
        });
        self.ui.set_can_reveal_report(self.can_show_report());
    }
    fn clear_selection(&self) {
        self.ui.set_has_item(false);
        self.ui.set_can_copy_source(false);
        self.ui.set_can_copy_destination(false);
        self.ui.set_can_reveal(false);
        self.ui.set_can_reveal_original(false);
        self.ui.set_item_source_path(PathView::default());
        self.ui.set_item_destination_path(PathView::default());
        self.ui.set_item_status("".into());
        self.ui.set_item_headline("".into());
        self.ui.set_item_guidance("".into());
        self.ui.set_item_policy_notice("".into());
        self.ui
            .set_details(update_model(self.ui.get_details(), Vec::new()));
        self.ui
            .set_removals(update_model(self.ui.get_removals(), Vec::new()));
        self.ui
            .set_stripped(update_model(self.ui.get_stripped(), Vec::new()));
        self.ui
            .set_byte_counts(update_model(self.ui.get_byte_counts(), Vec::new()));
    }
    fn clear_items(&self) {
        self.ui.set_folder_column_needed(true);
        self.ui.set_messages_scroll_y(0.0);
        self.ui.set_selected_visible_row(-1);
        self.ui.set_selected_item(-1);
        self.ui
            .set_items(update_model(self.ui.get_items(), Vec::new()));
        self.clear_selection();
        *self.last_view.borrow_mut() = None;
    }
    fn clear_report(&self) {
        self.ui.set_review_label("Run Again…".into());
        self.ui.set_report_loading(false);
        self.ui.set_report_details("".into());
        self.ui.set_run_notice("".into());
        self.ui
            .set_report_diagnostics(update_model(self.ui.get_report_diagnostics(), Vec::new()));
        self.ui
            .set_chips(update_model(self.ui.get_chips(), Vec::new()));
        self.clear_items();
        self.ui.set_run_label("".into());
        self.ui.set_run_title("".into());
        self.ui.set_run_status("".into());
        self.ui.set_run_context("".into());
        self.ui.set_report_running(false);
        self.ui.set_can_export(false);
        self.ui.set_can_dismiss(false);
        self.ui.set_can_create(false);
        self.ui.set_can_review(false);
        self.ui.set_can_reveal_output(false);
    }
}

fn chips(tally: state::Tally, selected: state::Filter) -> Vec<ChipView> {
    [
        (state::Filter::All, "All", tally.all, Tone::Neutral),
        (state::Filter::Created, "Created", tally.created, Tone::Ok),
        (
            state::Filter::Checked,
            "Checked",
            tally.checked,
            Tone::Neutral,
        ),
        (
            state::Filter::UpToDate,
            "Up to date",
            tally.up_to_date,
            Tone::Ok,
        ),
        (
            state::Filter::Attention,
            "Need attention",
            tally.attention,
            Tone::Attention,
        ),
        (
            state::Filter::NotProcessed,
            "Not processed",
            tally.not_processed,
            Tone::Stopped,
        ),
    ]
    .into_iter()
    .filter(|(filter, _, value, _)| *filter == state::Filter::All || *value > 0)
    .map(|(filter, label, value, tone)| ChipView {
        filter: filter.index(),
        label: label.into(),
        value: value as i32,
        active: filter == selected,
        tone,
    })
    .collect()
}
/// Native relative key shared by row sorting and full-run column analysis.
fn folder_key(source: &Path, request: &RequestSnapshot) -> Option<PathBuf> {
    if request.request().paths.iter().any(|path| path == source) {
        None
    } else {
        request
            .request()
            .paths
            .iter()
            .filter_map(|root| source.strip_prefix(root).ok())
            .max_by_key(|relative| std::cmp::Reverse(relative.components().count()))
            .map(|relative| {
                relative
                    .parent()
                    .filter(|parent| !parent.as_os_str().is_empty())
                    .unwrap_or(Path::new("."))
                    .to_owned()
            })
    }
}
/// Only a complete accepted inventory can establish that Folder is redundant.
fn folder_column_needed(report: &eml_app::RunReport, request: &RequestSnapshot) -> bool {
    let Some(start) = report.start().filter(|_| report.summary().is_some()) else {
        return true;
    };
    if report.items().len() != start.total as usize {
        return true;
    }
    let mut common = None;
    for index in 0..report.items().len() {
        let Some(source) = report.source_path(index) else {
            return true;
        };
        let Some(parent) = source.parent() else {
            return true;
        };
        let pair = (
            parent.to_owned(),
            folder_key(&source, request).unwrap_or_default(),
        );
        if common.as_ref().is_some_and(|common| *common != pair) {
            return true;
        }
        common = Some(pair);
    }
    false
}

pub(super) fn table_rows(
    report: &eml_app::RunReport,
    request: &RequestSnapshot,
    filter: state::Filter,
    sort: state::Sort,
) -> Vec<ItemView> {
    use std::ffi::OsString;
    struct Row {
        view: ItemView,
        source: PathBuf,
        folder: PathBuf,
        name: OsString,
    }
    let mut rows: Vec<_> = report
        .items()
        .iter()
        .enumerate()
        .filter(|(_, item)| filter.includes(view_model::category(item)))
        .map(|(index, item)| {
            let native = report.source_path(index);
            let source = native
                .clone()
                .unwrap_or_else(|| PathBuf::from(&item.source.display));
            let name = source.file_name().unwrap_or(source.as_os_str()).to_owned();
            let folder = folder_key(&source, request);
            let mut view = view_model::item(item);
            view.original_index = index as i32;
            view.folder = folder
                .as_ref()
                .map_or_else(
                    || "—".into(),
                    |folder| eml_protocol::TerminalPath::new(folder).to_string(),
                )
                .into();
            Row {
                view,
                source,
                folder: folder.unwrap_or_default(),
                name,
            }
        })
        .collect();
    rows.sort_by(|a, b| {
        use state::{Column, natural_cmp};
        let folder = || natural_cmp(a.folder.as_os_str(), b.folder.as_os_str());
        let name = || natural_cmp(&a.name, &b.name);
        let outcome = || {
            natural_cmp(
                std::ffi::OsStr::new(a.view.status.as_str()),
                std::ffi::OsStr::new(b.view.status.as_str()),
            )
        };
        let order = match sort.column {
            Column::Folder => folder().then_with(name),
            Column::Message => name().then_with(folder),
            Column::Outcome => outcome().then_with(folder).then_with(name),
        }
        .then_with(|| a.source.cmp(&b.source));
        if sort.descending {
            order.reverse()
        } else {
            order
        }
    });
    rows.into_iter().map(|row| row.view).collect()
}

fn folder_label(path: &Path, home: &Path) -> String {
    path.strip_prefix(home).map_or_else(
        |_| eml_protocol::TerminalPath::new(path).to_string(),
        |relative| format!("~/{}", eml_protocol::TerminalPath::new(relative)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{Engine, engine::general_purpose::STANDARD};
    fn report(sources: &[&str], total: usize, summary: bool, native: bool) -> eml_app::RunReport {
        let records: Vec<serde_json::Value> =
            include_str!("../../../tests/contract/protocol/created.jsonl")
                .replace("@VERSION@", eml_core::product::VERSION)
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
        let mut start = records[0].clone();
        start["total"] = total.into();
        let mut report = eml_app::RunReport::new(eml_core::product::VERSION);
        report.accept_frame(start.to_string().as_bytes()).unwrap();
        for (index, source) in sources.iter().enumerate() {
            let mut item = records[1].clone();
            item["index"] = index.into();
            item["source"]["display"] = (*source).into();
            item["source"]["native"]["base64"] = STANDARD.encode(source.as_bytes()).into();
            if !native {
                item["source"]["native"]["encoding"] = "windows-utf16le".into();
                item["source"]["native"]["base64"] = STANDARD
                    .encode(
                        source
                            .encode_utf16()
                            .flat_map(u16::to_le_bytes)
                            .collect::<Vec<_>>(),
                    )
                    .into();
            }
            report.accept_frame(item.to_string().as_bytes()).unwrap();
        }
        if summary {
            let mut end = records[2].clone();
            end["counts"]["created"] = sources.len().into();
            end["counts"]["not_processed"] = (total - sources.len()).into();
            if sources.is_empty() {
                end["exit_code"] = 1.into();
                end["run_error"] = serde_json::json!({"code":"NO_INPUTS", "phase":"collection", "detail":"No inputs", "section":null});
            } else if total > sources.len() {
                end["interrupted"] = true.into();
                end["exit_code"] = 130.into();
            }
            report.accept_frame(end.to_string().as_bytes()).unwrap();
        }
        report
    }
    #[test]
    fn folder_redundancy_requires_full_native_inventory_and_equal_sort_keys() {
        let request =
            RequestSnapshot::new(vec!["/one".into(), "/two".into()], None, RunMode::Create)
                .unwrap();
        for (sources, total, summary, native, needed) in [
            (vec!["/one/a.eml", "/one/b.eml"], 2, true, true, false),
            (vec!["/one/a.eml", "/two/b.eml"], 2, true, true, true),
            (vec!["/one/a.eml", "/one/sub/b.eml"], 2, true, true, true),
            (vec!["/one/a.eml"], 2, true, true, true),
            (vec!["/one/a.eml"], 1, false, true, true),
            (vec!["/one/a.eml"], 1, true, false, true),
            (vec![], 0, false, true, true),
            (vec![], 0, true, true, false),
        ] {
            let report = report(&sources, total, summary, native);
            assert_eq!(
                folder_column_needed(&report, &request),
                needed,
                "{sources:?}"
            );
            let _empty_filtered = table_rows(
                &report,
                &request,
                state::Filter::Attention,
                state::Sort::default(),
            );
            assert_eq!(folder_column_needed(&report, &request), needed);
        }
        let full = report(&["/one/a.eml", "/one/b.eml"], 2, true, true);
        let mixed_keys = RequestSnapshot::new(
            vec!["/one".into(), "/one/a.eml".into()],
            None,
            RunMode::Create,
        )
        .unwrap();
        assert!(folder_column_needed(&full, &mixed_keys));
        assert!(folder_column_needed(
            &eml_app::RunReport::new(eml_core::product::VERSION),
            &request
        ));
    }
}
