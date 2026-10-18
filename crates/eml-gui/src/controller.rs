//! View actions over the reusable session; no message or publication policy.
use crate::{
    native::{DialogResult, NativeDialog, OpenKind},
    preferences::DestinationPreference,
    view::MainWindow,
};
use eml_app::{
    DesktopSession, ReportExportOutcome, RequestSnapshot, RunId, TerminalReport, WorkerObservation,
};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::{
    path::PathBuf,
    thread::{self, JoinHandle},
    time::Instant,
};
fn terminal_label(completion: &eml_app::WorkerCompletion) -> &'static str {
    use eml_app::WorkerFailure;
    match completion.failure {
        Some(WorkerFailure::Launch) => {
            "The bundled worker could not start. Reinstall the app and try again."
        }
        Some(WorkerFailure::Identity) => {
            "The bundled worker identity differs. Reinstall the app before processing."
        }
        Some(WorkerFailure::Transport) => {
            "Worker results could not be fully verified. Earlier observations are retained."
        }
        Some(WorkerFailure::Unverified) => {
            "Worker resource settlement is unverified. Further processing is closed."
        }
        None if completion.signal.is_some() || completion.exit_code == Some(130) => {
            "Stopped. Completed results are retained."
        }
        None if completion.exit_code == Some(0) => "Finished.",
        None if completion.exit_code.is_some() => {
            "Finished with errors. Originals are unchanged; inspect the results."
        }
        None => "Stopped before processing.",
    }
}
fn item_label(item: &eml_protocol::records::ItemRecord) -> String {
    use eml_protocol::records::{ItemStatus, Publication};
    match item.status {
        ItemStatus::Created => format!("Created copy · Removed {} parts", item.removed.len()),
        ItemStatus::UpToDate => "Already up to date".into(),
        ItemStatus::Planned => "Planned".into(),
        ItemStatus::Failed => match item.publication.state {
            Publication::Published => "Copy published with an error".into(),
            Publication::Unknown => "Publication unconfirmed".into(),
            Publication::NotPublished => "Could not create copy".into(),
        },
    }
}
pub(crate) enum Action {
    Files,
    Folders,
    Destination,
    Beside,
    Stop,
    Force,
    Export,
    CancelExport,
    Dismiss,
    Select(usize),
    RemoveWaiting(usize),
    Previous,
    Next,
    Item(usize),
    PreviousParts,
    NextParts,
    Quit,
    Confirm,
    CancelConfirmation,
}
enum Confirmation {
    Force(RunId),
    Quit,
}
enum DialogPurpose {
    Selection,
    Destination,
    Export(TerminalReport),
}
struct Dialog {
    nonce: u64,
    purpose: DialogPurpose,
    native: NativeDialog,
}
struct ExportTask {
    _reservation: TerminalReport,
    control: eml_core::RunControl,
    actor: JoinHandle<ReportExportOutcome>,
}
pub(crate) struct Controller {
    session: DesktopSession,
    preference: DestinationPreference,
    selected: Option<RunId>,
    displayed: Vec<RunId>,
    waiting: Vec<RunId>,
    page: usize,
    item: usize,
    part_page: usize,
    view_key: Option<String>,
    run_rows: Vec<slint::SharedString>,
    waiting_rows: Vec<slint::SharedString>,
    nonce: u64,
    dialog: Option<Dialog>,
    confirmation: Option<Confirmation>,
    export: Option<ExportTask>,
    closing: bool,
    boundary_failed: bool,
    status: String,
}
impl Controller {
    pub(crate) fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let executable = std::env::current_exe()?;
        let macos = executable
            .parent()
            .ok_or("Application executable has no parent")?;
        // Shipping layout only. A development launch must use an owned bundle
        // with the exact private worker; there is no PATH or CLI fallback.
        let contents = macos
            .parent()
            .ok_or("Application bundle has no Contents directory")?;
        let helper = contents.join("Helpers/eml-worker");
        Ok(Self {
            session: DesktopSession::new(helper, env!("CARGO_PKG_VERSION").into()),
            preference: DestinationPreference::load(),
            selected: None,
            displayed: Vec::new(),
            waiting: Vec::new(),
            page: 0,
            item: 0,
            part_page: 0,
            view_key: None,
            run_rows: Vec::new(),
            waiting_rows: Vec::new(),
            nonce: 0,
            dialog: None,
            confirmation: None,
            export: None,
            closing: false,
            boundary_failed: false,
            status: "Choose messages or folders to begin.".into(),
        })
    }
    pub(crate) fn boundary_failure(&mut self, error: &str) {
        self.boundary_failed = true;
        self.status = error.into();
    }
    fn invalidate_dialog(&mut self) {
        self.nonce = self.nonce.saturating_add(1);
        if let Some(dialog) = &self.dialog {
            dialog.native.cancel();
        }
    }
    fn open(&mut self, window: &MainWindow, kind: OpenKind, purpose: DialogPurpose) {
        if self.dialog.is_some() || self.closing || self.confirmation.is_some() {
            return;
        }
        let initial = self.preference.destination().ok().flatten();
        match NativeDialog::open(window.window(), kind, initial) {
            Ok(native) => {
                self.dialog = Some(Dialog {
                    nonce: self.nonce,
                    purpose,
                    native,
                })
            }
            Err(error) => self.status = error.to_string(),
        }
    }
    pub(crate) fn action(&mut self, window: &MainWindow, action: Action) {
        if self.closing {
            if matches!(action, Action::CancelExport)
                && let Some(export) = &self.export
            {
                export.control.request_stop();
            }
            return;
        }
        if self.confirmation.is_some()
            && !matches!(
                action,
                Action::Confirm | Action::CancelConfirmation | Action::Quit
            )
        {
            return;
        }
        match action {
            Action::Files if !self.boundary_failed => {
                self.open(window, OpenKind::Files, DialogPurpose::Selection)
            }
            Action::Folders if !self.boundary_failed => {
                self.open(window, OpenKind::Folders, DialogPurpose::Selection)
            }
            Action::Destination if !self.boundary_failed => {
                self.open(window, OpenKind::Destination, DialogPurpose::Destination)
            }
            Action::Beside if self.dialog.is_none() && !self.boundary_failed => {
                match self.preference.choose(None) {
                    Ok(()) => {
                        self.status = "Later selections will save beside their originals.".into()
                    }
                    Err(error) => self.status = error.to_string(),
                }
            }
            Action::Stop => {
                if let Some(control) = self.session.control()
                    && control
                        .intent(Instant::now())
                        .is_ok_and(|intent| intent.interrupts == 0)
                {
                    match control.request_stop() {
                        Ok(_) => {
                            self.status =
                                "Stop requested; waiting for actual worker observations.".into()
                        }
                        Err(error) => self.boundary_failure(&error.to_string()),
                    }
                }
            }
            Action::Force => {
                if let Some(id) = self.session.active()
                    && self.session.control_for(id).is_some_and(|control| {
                        control
                            .intent(Instant::now())
                            .is_ok_and(|intent| intent.interrupts == 1)
                    })
                {
                    self.confirmation = Some(Confirmation::Force(id));
                }
            }
            Action::Quit => {
                self.invalidate_dialog();
                self.confirmation = Some(Confirmation::Quit);
            }
            Action::CancelConfirmation => {
                self.confirmation = None;
            }
            Action::Confirm => {
                match self.confirmation.take() {
                    Some(Confirmation::Force(id)) => {
                        if let Some(control) = self.session.control_for(id)
                            && let Err(error) = control.request_stop()
                        {
                            self.boundary_failure(&error.to_string());
                        }
                    }
                    Some(Confirmation::Quit) => {
                        self.closing = true;
                        self.invalidate_dialog();
                        if let Some(export) = &self.export {
                            export.control.request_stop();
                        }
                        if let Err(error) = self.session.confirm_shutdown(Instant::now()) {
                            self.boundary_failure(&error.to_string());
                        }
                        self.status = "Quitting: waiting for owned worker, dialog and report-export settlement.".into();
                    }
                    None => {}
                }
            }
            Action::Select(index) => {
                if let Some(id) = self.displayed.get(index) {
                    self.selected = Some(*id);
                    self.page = 0;
                    self.item = 0;
                    self.part_page = 0;
                }
            }
            Action::RemoveWaiting(index) => {
                if let Some(id) = self.waiting.get(index) {
                    self.session.remove_waiting(*id);
                }
            }
            Action::Dismiss => {
                if let Some(id) = self.selected
                    && self.session.dismiss(id)
                {
                    self.selected = self.session.reports().last().map(|report| report.id());
                    self.page = 0;
                }
            }
            Action::Export if self.export.is_none() && self.dialog.is_none() => {
                let captured = self
                    .session
                    .reports()
                    .iter()
                    .find(|report| Some(report.id()) == self.selected)
                    .and_then(|report| report.export_snapshot());
                if let Some(report) = captured {
                    match NativeDialog::save(window.window()) {
                        Ok(native) => {
                            self.dialog = Some(Dialog {
                                nonce: self.nonce,
                                purpose: DialogPurpose::Export(report),
                                native,
                            })
                        }
                        Err(error) => self.status = error.to_string(),
                    }
                }
            }
            Action::CancelExport => {
                if let Some(export) = &self.export {
                    export.control.request_stop();
                }
            }
            Action::Previous => {
                self.page = self.page.saturating_sub(1);
                self.item = self.page * 64;
                self.part_page = 0;
            }
            Action::Next => {
                self.page = self.page.saturating_add(1);
                self.item = self.page.saturating_mul(64);
                self.part_page = 0;
            }
            Action::Item(index) if index < 64 => {
                self.item = self.page.saturating_mul(64).saturating_add(index);
                self.part_page = 0;
            }
            Action::PreviousParts => {
                self.part_page = self.part_page.saturating_sub(1);
            }
            Action::NextParts => {
                self.part_page = self.part_page.saturating_add(1);
            }
            _ => {}
        }
    }
    fn selection(&mut self, paths: Vec<PathBuf>) {
        let destination = match self.preference.destination() {
            Ok(path) => path.map(ToOwned::to_owned),
            Err(error) => {
                self.status = error.to_string();
                return;
            }
        };
        match RequestSnapshot::new(paths, destination)
            .map_err(|error| error.to_string())
            .and_then(|request| {
                self.session
                    .enqueue(request)
                    .map_err(|error| error.to_string())
            }) {
            Ok(id) => self.status = format!("Run {id} queued with its captured copy destination."),
            Err(error) => self.status = error,
        }
    }
    fn start_export(&mut self, report: TerminalReport, destination: PathBuf) {
        let control = eml_core::RunControl::default();
        let background_control = control.clone();
        let captured = report.clone();
        match thread::Builder::new()
            .name("report-export".into())
            .spawn(move || eml_app::export_report(&captured, &destination, &background_control))
        {
            Ok(actor) => {
                self.export = Some(ExportTask {
                    _reservation: report,
                    control,
                    actor,
                });
                self.status =
                    "Exporting technical report; existing files will not be replaced.".into();
            }
            Err(error) => self.status = format!("Could not start report export: {error}"),
        }
    }
    /// Applies bounded view models before acknowledging a terminal run. This
    /// acknowledgement makes no claim about physical frame presentation.
    pub(crate) fn tick(&mut self, window: &MainWindow) -> bool {
        if let Some(result) = self
            .dialog
            .as_ref()
            .and_then(|dialog| dialog.native.take_result(window.window()))
        {
            let dialog = self.dialog.take().expect("observed owned dialog");
            if dialog.nonce == self.nonce && !self.closing && self.confirmation.is_none() {
                match result {
                    DialogResult::Chosen(paths) => match dialog.purpose {
                        DialogPurpose::Selection => self.selection(paths),
                        DialogPurpose::Destination => {
                            if paths.len() == 1 {
                                match self.preference.choose(paths.into_iter().next()) {
                                    Ok(()) => {
                                        self.status =
                                            "Copy destination changed for later selections.".into()
                                    }
                                    Err(error) => self.status = error.to_string(),
                                }
                            }
                        }
                        DialogPurpose::Export(report) => {
                            if paths.len() == 1 && self.export.is_none() {
                                self.start_export(
                                    report,
                                    paths.into_iter().next().expect("one selected path"),
                                );
                            }
                        }
                    },
                    DialogResult::Cancelled => {}
                    DialogResult::Failed(error) => self.status = error.to_string(),
                }
            }
        }
        if self
            .export
            .as_ref()
            .is_some_and(|export| export.actor.is_finished())
        {
            let export = self.export.take().expect("finished owned export");
            match export.actor.join() {
                Ok(outcome) if outcome.usable() => self.status = "Technical report saved and read back successfully.".into(),
                Ok(outcome) => self.status = format!("Report export: publication {:?}; address {:?}; final verification {:?}; file sync {:?}; namespace sync {:?}; cleanup {:?}. {}", outcome.publication, outcome.address_verified, outcome.final_file_verified, outcome.file_sync, outcome.namespace_sync, outcome.cleanup, outcome.failures.join("; ")),
                Err(_) => self.boundary_failure("Report export panicked; its filesystem outcome is unverified. Inspect the chosen destination before trying again."),
            }
            // The extra reservation is released only after task join and after
            // the task's serialized buffer has been destroyed.
            drop(export._reservation);
        }
        let completed = match self.session.poll() {
            Some(WorkerObservation::Settled(completion)) => {
                self.status = terminal_label(&completion).into();
                self.session.active()
            }
            Some(WorkerObservation::Pending {
                warning: Some(warning),
            }) => {
                self.status = warning;
                None
            }
            _ => None,
        };
        let applied = self.render(window);
        if applied
            && let Some(id) = completed
            && let Err(error) = self.session.acknowledge_completion(id)
        {
            self.boundary_failure(&error.to_string());
        }
        if !self.closing && !self.boundary_failed && !self.session.blocked() {
            match self.session.start_next() {
                Ok(true) => {
                    self.selected = self.session.active();
                    self.page = 0;
                }
                Ok(false) => {}
                Err(eml_app::SessionError::Queue(eml_app::QueueError::ReportSpace)) => {
                    if self.session.waiting() > 0 {
                        self.status =
                            "Queue paused: dismiss a retained report to make room.".into();
                    }
                }
                Err(error) => self.status = error.to_string(),
            }
        }
        self.closing
            && self.session.worker_shutdown_settled()
            && self.dialog.is_none()
            && self.export.is_none()
    }
    fn render(&mut self, window: &MainWindow) -> bool {
        // Feed measured logical size into responsive layout. A Slint layout
        // constraint depending on its own Window dimensions forms a binding cycle.
        let size = window.window().size();
        let scale = window.window().scale_factor();
        window.set_compact_layout(size.width as f32 / scale < 900.0);
        window.set_short_layout(size.height as f32 / scale < 620.0);
        let mut applied = true;
        let mut runs = Vec::new();
        self.displayed.clear();
        for report in self.session.reports().iter().rev() {
            self.displayed.push(report.id());
            let state = report.completion().map_or("Running", |completion| {
                if completion.failure.is_some() {
                    "Needs attention"
                } else if completion.signal.is_some() || completion.exit_code == Some(130) {
                    "Stopped"
                } else if completion.exit_code == Some(0) {
                    "Finished"
                } else {
                    "Finished with errors"
                }
            });
            let facts = match report.report().inspect(|facts| {
                facts.summary().map_or_else(
                    || format!("{} received", facts.items().len()),
                    |summary| {
                        format!(
                            "{} created · {} up to date\n{} failed · {} not processed",
                            summary.counts.created,
                            summary.counts.up_to_date,
                            summary.counts.failed,
                            summary.counts.not_processed
                        )
                    },
                )
            }) {
                Ok(Some(facts)) => facts,
                _ => {
                    if self.session.active() == Some(report.id()) {
                        applied = false;
                    }
                    "Model busy / unavailable".into()
                }
            };
            runs.push(format!("Run {}\n{state}\n{facts}", report.id()).into());
        }
        self.waiting.clear();
        let waiting = self
            .session
            .waiting_requests()
            .map(|(id, request)| {
                self.waiting.push(id);
                format!(
                    "Run {id}: Not started — {} selections; destination {:?}",
                    request.request().paths.len(),
                    request.request().output
                )
                .into()
            })
            .collect::<Vec<slint::SharedString>>();
        if runs != self.run_rows {
            self.run_rows = runs.clone();
            window.set_runs(ModelRc::new(VecModel::from(runs)));
        }
        if waiting != self.waiting_rows {
            self.waiting_rows = waiting.clone();
            window.set_waiting_runs(ModelRc::new(VecModel::from(waiting)));
        }
        window.set_selected_run(
            self.displayed
                .iter()
                .position(|id| Some(*id) == self.selected)
                .map_or(-1, |index| index as i32),
        );
        let idle_dialog = self.dialog.is_none() && self.confirmation.is_none();
        window.set_admission_ready(
            !self.closing
                && !self.boundary_failed
                && !self.session.blocked()
                && idle_dialog
                && self.session.waiting() < 8
                && self.preference.destination().is_ok(),
        );
        window.set_destination_editable(!self.closing && !self.boundary_failed && idle_dialog);
        let mut destination = self.preference.destination().map_or_else(
            |error| error.to_string(),
            |path| {
                path.map_or_else(
                    || "Beside each original".into(),
                    |path| path.to_string_lossy().into_owned(),
                )
            },
        );
        if self.preference.session_only() {
            destination.push_str(" (this session only)");
        }
        window.set_destination(destination.into());
        window.set_activity(self.status.clone().into());
        window.set_running(self.session.active().is_some() && !self.closing);
        window.set_stop_available(
            !self.closing
                && self.session.control().is_some_and(|control| {
                    control
                        .intent(Instant::now())
                        .is_ok_and(|intent| intent.interrupts == 0 && !intent.reaped)
                }),
        );
        window.set_force_available(
            !self.closing
                && self.session.control().is_some_and(|control| {
                    control
                        .intent(Instant::now())
                        .is_ok_and(|intent| intent.interrupts == 1 && !intent.reaped)
                }),
        );
        window.set_export_running(self.export.is_some());
        let selected = self
            .session
            .reports()
            .iter()
            .find(|report| Some(report.id()) == self.selected);
        window.set_report_selected(
            !self.closing
                && idle_dialog
                && self.export.is_none()
                && selected.is_some_and(|report| report.completion().is_some()),
        );
        if let Ok((dark, contrast)) = crate::native::appearance() {
            window.set_dark_appearance(dark);
            window.set_high_contrast(contrast);
        }
        if let Some(report) = selected {
            let completion = report.completion();
            let inspected = report.report().inspect(|facts| {
                let pages = facts.items().len().div_ceil(64).max(1);
                self.page = self.page.min(pages - 1);
                self.item = self.item.min(facts.items().len().saturating_sub(1));
                let key = format!("{}:{}:{}:{}:{}:{}:{}:{:?}:{}", report.id(), self.page, self.item, self.part_page, facts.items().len(), facts.start().is_some(), facts.summary().is_some(), facts.failure(), completion.is_some());
                if self.view_key.as_ref() == Some(&key) {return None;}
                let mut summary = format!("Run {} · ", report.id());
                match facts.start() {
                    Some(start) => summary.push_str(&format!("{} collected · {} results received", start.total, facts.items().len())),
                    None => summary.push_str("Scanning files…"),
                }
                if let Some(counts) = facts.summary().map(|summary| &summary.counts) {
                    summary.push_str(&format!("\n{} created · {} up to date · {} failed · {} not processed", counts.created, counts.up_to_date, counts.failed, counts.not_processed));
                }
                if let Some(completion) = completion {summary.push_str(&format!("\n{}", terminal_label(completion)));}
                summary.push_str(&format!("\nItems page {} of {pages}", self.page + 1));
                let rows = facts.items().iter().skip(self.page * 64).take(64).map(|item| {
                    let name: String = item.source.display.rsplit('/').next().unwrap_or("Message").chars().take(180).collect();
                    format!("{} · {}", name, item_label(item)).into()
                }).collect::<Vec<slint::SharedString>>();
                let mut detail = String::new();
                let mut technical = format!("Protocol failure: {:?}\n", facts.failure());
                if let Some(completion) = completion {technical.push_str(&format!("Worker exit {:?}; signal {:?}; failure {:?}; settled {}; reusable {}\n{}\nStderr{}:\n{}\n", completion.exit_code, completion.signal, completion.failure, completion.settled, completion.reusable, completion.faults.join("; "), if completion.stderr_truncated {" (truncated)"} else {""}, String::from_utf8_lossy(&completion.stderr)));}
                if let Some(item) = facts.items().get(self.item) {
                    detail.push_str(&format!("{}\nSource: {}\nCopy: {}\n", item_label(item), item.source.display, item.destination.value().map_or("—", |path| path.display.as_str())));
                    if let Some(saved) = facts.saved_bytes(self.item) {detail.push_str(&format!("Saved {saved} bytes\n"));}
                    if let Some(error) = item.error.value() {detail.push_str(&format!("{}\nOriginal message unchanged.\n", error.detail));}
                    let pages = item.removed.len().div_ceil(64).max(1);
                    self.part_page = self.part_page.min(pages - 1);
                    detail.push_str(&format!("Removed parts page {} of {pages}\n", self.part_page + 1));
                    for part in item.removed.iter().skip(self.part_page * 64).take(64) {
                        detail.push_str(&format!("{} · {} · {} bytes · {:?} · section {}\n", part.filename.value().map_or("Unnamed part", String::as_str), part.content_type, part.bytes, part.reason, part.section));
                    }
                    if !item.stripped_fields.is_empty() {detail.push_str(&format!("Removed stale fields: {}\n", item.stripped_fields.join(", ")));}
                    technical.push_str(&format!("Publication: {:#?}\nExisting copy: {:#?}\nItem error: {:#?}\n", item.publication, item.existing_observation.value(), item.error.value()));
                }
                let artwork = if completion.is_none() {1} else if facts.failure().is_some() || completion.is_some_and(|completion| completion.failure.is_some()) || facts.summary().is_some_and(|summary| summary.counts.failed > 0) {5} else if facts.summary().is_some_and(|summary| summary.interrupted) || completion.is_some_and(|completion| completion.signal.is_some() || completion.exit_code == Some(130)) {3} else {2};
                Some((key, summary, rows, detail, technical, artwork))
            });
            match inspected {
                Ok(Some(Some((key, summary, rows, detail, technical, artwork)))) => {
                    // Native/UI calls occur only after releasing the model lock.
                    self.view_key = Some(key);
                    window.set_report_text(summary.into());
                    window.set_items(ModelRc::new(VecModel::from(rows)));
                    window.set_item_text(detail.into());
                    window.set_technical_text(technical.into());
                    window.set_artwork_state(artwork);
                }
                Ok(Some(None)) => {}
                _ => {
                    if self.session.active() == Some(report.id()) {
                        applied = false;
                    }
                }
            }
        } else {
            self.view_key = None;
            window.set_report_text("".into());
            window.set_item_text("".into());
            window.set_technical_text("".into());
            window.set_items(ModelRc::new(VecModel::from(
                Vec::<slint::SharedString>::new(),
            )));
            window.set_artwork_state(0);
        }
        window.set_confirm_visible(self.confirmation.is_some());
        window.set_confirm_text(match self.confirmation {
            Some(Confirmation::Quit) => "Quit and cancel waiting runs? Active processing will be stopped and owned resources must settle before the app exits.",
            Some(Confirmation::Force(_)) => "Force Stop this run? Earlier observations remain; an in-flight publication may already have taken effect.",
            None => "",
        }.into());
        applied
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn published_failed_and_unknown_present_distinct_copy_knowledge() {
        use eml_protocol::records::{EventRecord, decode_record};
        for (fixture, expected) in [
            (
                include_bytes!("../../../tests/contract/protocol/prepublication-failure.jsonl")
                    .as_slice(),
                "Could not create copy",
            ),
            (
                include_bytes!("../../../tests/contract/protocol/published-with-error.jsonl")
                    .as_slice(),
                "Copy published with an error",
            ),
            (
                include_bytes!("../../../tests/contract/protocol/unknown-publication.jsonl")
                    .as_slice(),
                "Publication unconfirmed",
            ),
        ] {
            let item = fixture
                .split(|byte| *byte == b'\n')
                .filter(|line| !line.is_empty())
                .find_map(|line| match decode_record(line).unwrap() {
                    EventRecord::Item(item) => Some(item),
                    _ => None,
                })
                .unwrap();
            assert_eq!(item_label(&item), expected);
        }
    }
}
