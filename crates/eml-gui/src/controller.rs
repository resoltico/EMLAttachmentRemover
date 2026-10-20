//! View actions over the reusable session; no message or publication policy.
use crate::{
    native::{DialogResult, NativeDialog, OpenKind},
    preferences::DestinationPreference,
    presentation::{self, ReportPosition, UserNotice, display_lines, terminal_label},
    view::MainWindow,
};
use eml_app::{
    DesktopSession, ReportExportOutcome, RequestSnapshot, RunId, TerminalReport, WorkerObservation,
};
use eml_protocol::DisplayText;
use slint::{ComponentHandle, ModelRc, VecModel};
use std::{
    path::PathBuf,
    thread::{self, JoinHandle},
    time::Instant,
};
pub(crate) enum Action {
    Files,
    Folders,
    Destination,
    Beside,
    Stop,
    Force,
    Export,
    Reveal,
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
    DismissNotice,
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
    accept_result: bool,
    purpose: DialogPurpose,
    native: NativeDialog,
}
struct ExportTask {
    reservation: TerminalReport,
    control: eml_core::RunControl,
    actor: JoinHandle<ReportExportOutcome>,
}
enum RevealTask {
    Inspect {
        path: PathBuf,
        actor: JoinHandle<std::io::Result<bool>>,
    },
    Native(crate::native::NativeReveal),
}
pub(crate) struct Controller {
    session: DesktopSession,
    preference: DestinationPreference,
    selected: Option<RunId>,
    displayed: Vec<RunId>,
    waiting: Vec<RunId>,
    position: ReportPosition,
    view_key: Option<String>,
    run_rows: Vec<slint::SharedString>,
    waiting_rows: Vec<slint::SharedString>,
    dialog: Option<Dialog>,
    confirmation: Option<Confirmation>,
    export: Option<ExportTask>,
    reveal: Option<RevealTask>,
    closing_since: Option<Instant>,
    admission_failure: Option<String>,
    notice: Option<UserNotice>,
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
            position: ReportPosition::default(),
            view_key: None,
            run_rows: Vec::new(),
            waiting_rows: Vec::new(),
            dialog: None,
            confirmation: None,
            export: None,
            reveal: None,
            closing_since: None,
            admission_failure: None,
            notice: None,
            status: "Choose messages or folders to begin.".into(),
        })
    }
    pub(crate) fn boundary_failure(&mut self, error: &str) {
        self.admission_failure
            .get_or_insert_with(|| error.chars().take(1000).collect());
        self.status = error.into();
    }
    fn retain_report_failure(&mut self) {
        if self.admission_failure.is_none() {
            self.boundary_failure("A report could not be safely inspected. Further processing is closed; inspect the unavailable report and its Technical details.");
        }
    }
    pub(crate) fn system_notice(&mut self, message: &str) {
        self.notice = Some(UserNotice::message(message));
    }
    pub(crate) fn ingress(&mut self, batch: Result<Vec<PathBuf>, crate::native::NativeError>) {
        if self.closing_since.is_some()
            || self.confirmation.is_some()
            || self.admission_failure.is_some()
        {
            self.system_notice("Selection not started while confirmation, shutdown or a platform boundary failure closes admission. Retry from the main window when available.");
            return;
        }
        match batch {
            Ok(paths) => self.selection(paths),
            Err(error) => self.system_notice(&format!("Selection not started: {error}")),
        }
    }
    fn invalidate_dialog(&mut self) {
        if let Some(dialog) = &mut self.dialog {
            dialog.accept_result = false;
            dialog.native.cancel();
        }
    }
    fn open(&mut self, window: &MainWindow, kind: OpenKind, purpose: DialogPurpose) {
        if self.dialog.is_some() || self.closing_since.is_some() || self.confirmation.is_some() {
            return;
        }
        let initial = self.preference.destination().ok().flatten();
        match NativeDialog::open(window.window(), kind, initial) {
            Ok(native) => {
                self.dialog = Some(Dialog {
                    accept_result: true,
                    purpose,
                    native,
                })
            }
            Err(error) => self.system_notice(&error.to_string()),
        }
    }
    pub(crate) fn action(&mut self, window: &MainWindow, action: Action) {
        if self.closing_since.is_some() {
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
            Action::DismissNotice => self.notice = None,
            Action::Files if self.admission_failure.is_none() => {
                self.open(window, OpenKind::Files, DialogPurpose::Selection)
            }
            Action::Folders if self.admission_failure.is_none() => {
                self.open(window, OpenKind::Folders, DialogPurpose::Selection)
            }
            Action::Destination if self.admission_failure.is_none() => {
                self.open(window, OpenKind::Destination, DialogPurpose::Destination)
            }
            Action::Beside if self.dialog.is_none() && self.admission_failure.is_none() => {
                match self.preference.choose(None) {
                    Ok(()) => {
                        self.status = "Later selections will save beside their originals.".into()
                    }
                    Err(error) => self.system_notice(&error.to_string()),
                }
            }
            Action::Stop => self.request_stop(),
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
                if self.closing_since.is_none() {
                    self.invalidate_dialog();
                    if let Err(error) = crate::native::restore(window.window()) {
                        self.system_notice(&format!("Quit confirmation window: {error}"));
                    }
                    self.confirmation = Some(Confirmation::Quit);
                }
            }
            Action::CancelConfirmation => {
                self.confirmation = None;
            }
            Action::Confirm => self.confirm_action(),
            Action::Select(index) => {
                if let Some(id) = self.displayed.get(index) {
                    self.selected = Some(*id);
                    self.position.page = 0;
                    self.position.item = 0;
                    self.position.part_page = 0;
                }
            }
            Action::RemoveWaiting(index) => {
                if let Some(id) = self.waiting.get(index).copied()
                    && self.session.remove_waiting(id)
                {
                    self.status = format!("Run {id} removed from the waiting queue.");
                }
            }
            Action::Dismiss => {
                if let Some(id) = self.selected
                    && self.session.dismiss(id)
                {
                    self.selected = self.session.reports().last().map(|report| report.id());
                    self.position.page = 0;
                }
            }
            Action::Reveal if self.reveal.is_none() => self.start_reveal(),
            Action::Export if self.export.is_none() && self.dialog.is_none() => {
                self.choose_export(window)
            }
            Action::CancelExport => {
                if let Some(export) = &self.export {
                    export.control.request_stop();
                }
            }
            Action::Previous
            | Action::Next
            | Action::Item(_)
            | Action::PreviousParts
            | Action::NextParts => self.navigate(action),
            _ => {}
        }
    }
    fn request_stop(&mut self) {
        if let Some(control) = self.session.control()
            && control
                .intent(Instant::now())
                .is_ok_and(|intent| intent.interrupts == 0)
        {
            match control.request_stop() {
                Ok(_) => {
                    self.status = "Stop requested; waiting for actual worker observations.".into()
                }
                Err(error) => self.boundary_failure(&error.to_string()),
            }
        }
    }
    fn confirm_action(&mut self) {
        match self.confirmation.take() {
            Some(Confirmation::Force(id)) => {
                if let Some(control) = self.session.control_for(id)
                    && let Err(error) = control.request_stop()
                {
                    self.boundary_failure(&error.to_string());
                }
            }
            Some(Confirmation::Quit) => {
                let confirmed = Instant::now();
                self.closing_since = Some(confirmed);
                self.invalidate_dialog();
                if let Some(export) = &self.export {
                    export.control.request_stop();
                }
                if let Err(error) = self.session.confirm_shutdown(confirmed) {
                    self.boundary_failure(&error.to_string());
                }
                self.status =
                    "Quitting: waiting for owned worker, dialog and report-export settlement."
                        .into();
            }
            None => {}
        }
    }
    fn start_reveal(&mut self) {
        let captured = self
            .session
            .reports()
            .iter()
            .find(|report| Some(report.id()) == self.selected)
            .and_then(|report| {
                report
                    .report()
                    .inspect(|facts| facts.reveal_path(self.position.item))
                    .ok()
                    .flatten()
                    .flatten()
            });
        if let Some(path) = captured {
            let check = path.clone();
            match thread::Builder::new()
                .name("finder-reveal".into())
                .spawn(move || check.try_exists())
            {
                Ok(actor) => self.reveal = Some(RevealTask::Inspect { path, actor }),
                Err(error) => {
                    self.system_notice(&format!("Could not start Finder navigation: {error}"))
                }
            }
        }
    }
    fn choose_export(&mut self, window: &MainWindow) {
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
                        accept_result: true,
                        purpose: DialogPurpose::Export(report),
                        native,
                    })
                }
                Err(error) => self.system_notice(&error.to_string()),
            }
        }
    }
    fn navigate(&mut self, action: Action) {
        match action {
            Action::Previous => {
                self.position.page = self.position.page.saturating_sub(1);
                self.position.item = self.position.page.saturating_mul(presentation::PAGE_SIZE);
                self.position.part_page = 0;
            }
            Action::Next => {
                self.position.page = self.position.page.saturating_add(1);
                self.position.item = self.position.page.saturating_mul(presentation::PAGE_SIZE);
                self.position.part_page = 0;
            }
            Action::Item(index) if index < presentation::PAGE_SIZE => {
                self.position.item = self
                    .position
                    .page
                    .saturating_mul(presentation::PAGE_SIZE)
                    .saturating_add(index);
                self.position.part_page = 0;
            }
            Action::PreviousParts => {
                self.position.part_page = self.position.part_page.saturating_sub(1);
            }
            Action::NextParts => {
                self.position.part_page = self.position.part_page.saturating_add(1);
            }
            _ => {}
        }
    }
    fn selection(&mut self, paths: Vec<PathBuf>) {
        let destination = match self.preference.destination() {
            Ok(path) => path.map(ToOwned::to_owned),
            Err(error) => {
                self.system_notice(&format!("Selection not started: {error}"));
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
            Err(error) => self.system_notice(&format!("Selection not started: {error}")),
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
                    reservation: report,
                    control,
                    actor,
                });
                self.status =
                    "Exporting technical report; existing files will not be replaced.".into();
            }
            Err(error) => self.system_notice(&format!("Could not start report export: {error}")),
        }
    }
    /// Applies bounded view models before acknowledging a terminal run. This
    /// acknowledgement makes no claim about physical frame presentation.
    pub(crate) fn tick(&mut self, window: &MainWindow) -> bool {
        self.settle_reveal();
        self.settle_dialog(window);
        self.settle_export();
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
        if self.closing_since.is_none()
            && self.admission_failure.is_none()
            && !self.session.blocked()
        {
            match self.session.start_next() {
                Ok(true) => {
                    self.selected = self.session.active();
                    self.position = ReportPosition::default();
                    if let Some(id) = self.selected {
                        self.status = format!("Processing run {id}…");
                    }
                }
                Ok(false) => {}
                Err(eml_app::SessionError::Queue(eml_app::QueueError::ReportSpace)) => {}
                Err(error) => self.system_notice(&error.to_string()),
            }
        }
        self.closing_since.is_some()
            && self.session.worker_shutdown_settled()
            && self.dialog.is_none()
            && self.export.is_none()
            && self.reveal.is_none()
    }
    fn settle_reveal(&mut self) {
        let reveal_ready = self.reveal.as_ref().is_some_and(|task| match task {
            RevealTask::Inspect { actor, .. } => actor.is_finished(),
            RevealTask::Native(task) => task.result().is_some(),
        });
        if reveal_ready {
            let task = self.reveal.take().expect("finished owned Finder task");
            match task {
                RevealTask::Inspect { path, actor } => match actor.join() {
                    Ok(Ok(true)) if self.closing_since.is_none() => {
                        match crate::native::reveal(path) {
                            Ok(task) => self.reveal = Some(RevealTask::Native(task)),
                            Err(error) => self.system_notice(&format!("Finder navigation: {error}")),
                        }
                    }
                    Ok(Ok(true)) => {}
                    Ok(Ok(false)) => self.system_notice("The earlier reported copy is unavailable at that address. Its receipt remains unchanged; Finder navigation does not reverify it."),
                    Ok(Err(error)) => self.system_notice(&format!("Finder navigation is unavailable: {error}. The earlier receipt remains unchanged.")),
                    Err(_) => self.boundary_failure("Finder navigation task failed; its owned I/O settlement could not be verified."),
                },
                RevealTask::Native(task) => {
                    if let Err(error) = task.result().expect("observed native Finder completion") {
                        self.system_notice(&format!("Finder navigation: {error}"));
                    }
                }
            }
        }
    }
    fn settle_dialog(&mut self, window: &MainWindow) {
        if let Some(result) = self
            .dialog
            .as_ref()
            .and_then(|dialog| dialog.native.take_result(window.window()))
        {
            let dialog = self.dialog.take().expect("observed owned dialog");
            if dialog.accept_result && self.closing_since.is_none() && self.confirmation.is_none() {
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
                                    Err(error) => self.system_notice(&error.to_string()),
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
                    DialogResult::Failed(error) => self.system_notice(&error.to_string()),
                }
            }
        }
    }
    fn settle_export(&mut self) {
        if self
            .export
            .as_ref()
            .is_some_and(|export| export.actor.is_finished())
        {
            let export = self.export.take().expect("finished owned export");
            match export.actor.join() {
                Ok(outcome) => {
                    self.status = "Report export finished.".into();
                    self.notice = Some(presentation::export_notice(export.reservation.id(), &outcome));
                }
                Err(_) => self.boundary_failure("Report export panicked; its filesystem outcome is unverified. Inspect the chosen destination before trying again."),
            }
            // The extra reservation is released only after task join and after
            // the task's serialized buffer has been destroyed.
            drop(export.reservation);
        }
    }
    fn render(&mut self, window: &MainWindow) -> bool {
        let size = window.window().size();
        let scale = window.window().scale_factor();
        window.set_compact_layout(size.width as f32 / scale < 900.0);
        window.set_short_layout(size.height as f32 / scale < 620.0);
        let runs_applied = self.render_runs(window);
        self.render_destination(window);
        self.render_activity(window);
        self.render_actions(window);
        let appearance = crate::native::appearance().ok();
        if let Some((dark, contrast)) = appearance {
            window.set_dark_appearance(dark);
            window.set_high_contrast(contrast);
        }
        let selection_applied = self.render_selected_report(window, appearance);
        self.render_confirmation(window);
        runs_applied && selection_applied
    }
    fn render_runs(&mut self, window: &MainWindow) -> bool {
        let mut applied = true;
        let mut rows = Vec::new();
        let mut unavailable = false;
        self.displayed.clear();
        for report in self.session.reports().iter().rev() {
            self.displayed.push(report.id());
            let row = report
                .report()
                .inspect(|facts| presentation::run_row(report.id(), facts, report.completion()));
            match row {
                Ok(Some(row)) => rows.push(row.into()),
                Err(_) => {
                    unavailable = true;
                    applied &= self.session.active() != Some(report.id())
                        || report.completion().is_some_and(|c| c.settled);
                    rows.push(format!("Run {}\nReport unavailable", report.id()).into());
                }
                Ok(None) => {
                    applied &= self.session.active() != Some(report.id());
                    rows.push(
                        format!("Run {}\nReport is busy; waiting for results.", report.id()).into(),
                    );
                }
            }
        }
        if unavailable {
            self.retain_report_failure();
        }
        self.waiting.clear();
        let waiting = self
            .session
            .waiting_requests()
            .map(|(id, request)| {
                self.waiting.push(id);
                presentation::waiting_row(id, request).into()
            })
            .collect::<Vec<slint::SharedString>>();
        if rows != self.run_rows {
            self.run_rows = rows.clone();
            let picker = rows
                .iter()
                .map(|row| row.as_str().replace('\n', " · ").into())
                .collect::<Vec<slint::SharedString>>();
            window.set_run_picker_labels(ModelRc::new(VecModel::from(picker)));
            window.set_runs(ModelRc::new(VecModel::from(rows)));
        }
        if waiting != self.waiting_rows {
            self.waiting_rows = waiting.clone();
            window.set_waiting_runs(ModelRc::new(VecModel::from(waiting)));
        }
        window.set_selected_run(
            self.displayed
                .iter()
                .position(|id| Some(*id) == self.selected)
                .map_or(-1, |i| i as i32),
        );
        applied
    }
    fn render_destination(&self, window: &MainWindow) {
        let mut destination = self.preference.destination().map_or_else(
            |e| e.to_string(),
            |path| {
                path.map_or_else(
                    || "Beside each original".into(),
                    |p| DisplayText::new(&p.to_string_lossy()).to_string(),
                )
            },
        );
        if self.preference.session_only() {
            destination.push_str(" (this session only)");
        }
        window.set_destination(destination.into());
    }
    fn current_activity(&self) -> String {
        let shutdown = self.closing_since.map(|since| {
            if since.elapsed() >= std::time::Duration::from_secs(2) {
                "Still waiting for owned worker, dialog or native I/O. The app will exit only after actual settlement."
            } else {
                "Quitting: waiting for processing, dialogs and report export to stop safely."
            }
        });
        if let Some(shutdown) = shutdown {
            return self.admission_failure.as_ref().map_or_else(
                || shutdown.into(),
                |failure| format!("{shutdown}\n{failure}"),
            );
        }
        if let Some(failure) = &self.admission_failure {
            return if failure == &self.status {
                failure.clone()
            } else {
                format!("{failure}\n{}", self.status)
            };
        }
        if self.session.blocked() {
            return format!(
                "Further processing is closed because worker lifecycle could not be verified. Retained reports remain available.\n{}",
                self.status
            );
        }
        if self.export.is_some() {
            return "Exporting technical report; existing files will not be replaced.".into();
        }
        match self.session.activity() {
            eml_app::SessionActivity::Running(id) => format!("Processing run {id}…"),
            eml_app::SessionActivity::StopRequested { id, repeated } => format!(
                "{} requested for run {id}; waiting for processing to stop.",
                if repeated { "Force Stop" } else { "Stop" }
            ),
            eml_app::SessionActivity::Settled(id) => self
                .session
                .reports()
                .iter()
                .find(|r| r.id() == id)
                .and_then(|r| r.completion())
                .map_or_else(|| self.status.clone(), |c| terminal_label(c).into()),
            eml_app::SessionActivity::WaitingForReportSpace => {
                "Queue paused: dismiss a retained report to make room.".into()
            }
            _ => self.status.clone(),
        }
    }
    fn render_activity(&self, window: &MainWindow) {
        window.set_activity(display_lines(&self.current_activity()).into());
        window.set_notice_text(self.notice.as_ref().map_or_else(
            || "".into(),
            |n| DisplayText::new(&n.message).to_string().into(),
        ));
        window.set_notice_details(
            self.notice
                .as_ref()
                .map_or_else(|| "".into(), |n| n.details.clone().into()),
        );
    }
    fn render_actions(&self, window: &MainWindow) {
        let idle_dialog = self.dialog.is_none() && self.confirmation.is_none();
        window.set_native_dialog_active(self.dialog.is_some());
        window.set_admission_ready(
            self.closing_since.is_none()
                && self.admission_failure.is_none()
                && !self.session.blocked()
                && idle_dialog
                && self.session.waiting() < eml_app::MAX_WAITING_REQUESTS
                && self.preference.destination().is_ok(),
        );
        window.set_destination_editable(
            self.closing_since.is_none() && self.admission_failure.is_none() && idle_dialog,
        );
        window.set_stop_available(
            self.closing_since.is_none()
                && self.session.control().is_some_and(|c| {
                    c.intent(Instant::now())
                        .is_ok_and(|i| i.interrupts == 0 && !i.reaped)
                }),
        );
        window.set_force_available(
            self.closing_since.is_none()
                && self.session.control().is_some_and(|c| {
                    c.intent(Instant::now())
                        .is_ok_and(|i| i.interrupts == 1 && !i.reaped)
                }),
        );
        window.set_export_running(self.export.is_some());
        let selected = self
            .session
            .reports()
            .iter()
            .find(|r| Some(r.id()) == self.selected);
        window.set_reveal_available(
            self.closing_since.is_none()
                && idle_dialog
                && self.reveal.is_none()
                && selected.is_some_and(|r| {
                    r.report()
                        .inspect(|f| f.reveal_path(self.position.item).is_some())
                        .ok()
                        .flatten()
                        .unwrap_or(false)
                }),
        );
        window.set_report_selected(
            self.closing_since.is_none()
                && idle_dialog
                && self.export.is_none()
                && selected.is_some_and(|r| r.completion().is_some()),
        );
    }
    fn render_selected_report(
        &mut self,
        window: &MainWindow,
        appearance: Option<(bool, bool)>,
    ) -> bool {
        let Some(report) = self
            .session
            .reports()
            .iter()
            .find(|r| Some(r.id()) == self.selected)
        else {
            self.clear_selected_report(window);
            return true;
        };
        let inspected = report.report().inspect(|facts| {
            let position = self.position.clamped(facts);
            let key = presentation::view_key(
                report.id(),
                facts,
                report.completion().is_some(),
                position,
                appearance,
            );
            let page = (self.view_key.as_ref() != Some(&key)).then(|| {
                presentation::report_page(
                    report.id(),
                    facts,
                    report.completion(),
                    position,
                    appearance,
                )
            });
            (key, position, page)
        });
        match inspected {
            Ok(Some((key, position, page))) => {
                self.position = position;
                if let Some(page) = page {
                    self.view_key = Some(key);
                    Self::apply_report_page(window, page);
                }
                true
            }
            Ok(None) => self.session.active() != Some(report.id()),
            Err(failure) => {
                let settled = report.completion().is_some_and(|c| c.settled);
                let page = presentation::unavailable_report_page(
                    report.id(),
                    report.completion(),
                    failure,
                    appearance,
                );
                self.view_key = None;
                Self::apply_report_page(window, page);
                self.retain_report_failure();
                // A failure view acknowledges only independently settled ownership.
                settled
            }
        }
    }
    fn apply_report_page(window: &MainWindow, page: presentation::ReportPage) {
        // Every native/UI call occurs after releasing the report model lock.
        window.set_report_text(page.summary.into());
        window.set_items(ModelRc::new(VecModel::from(
            page.rows
                .into_iter()
                .map(slint::SharedString::from)
                .collect::<Vec<_>>(),
        )));
        window.set_item_text(page.detail.into());
        window.set_technical_text(page.technical.into());
        window.set_artwork_state(page.artwork);
        window.set_previous_items_available(page.navigation.previous_items);
        window.set_next_items_available(page.navigation.next_items);
        window.set_previous_parts_available(page.navigation.previous_parts);
        window.set_next_parts_available(page.navigation.next_parts);
    }
    fn clear_selected_report(&mut self, window: &MainWindow) {
        self.view_key = None;
        window.set_previous_items_available(false);
        window.set_next_items_available(false);
        window.set_previous_parts_available(false);
        window.set_next_parts_available(false);
        window.set_report_text("".into());
        window.set_item_text("".into());
        window.set_technical_text("".into());
        window.set_items(ModelRc::new(VecModel::from(
            Vec::<slint::SharedString>::new(),
        )));
        window.set_artwork_state(crate::view::ArtworkState::Identity);
    }
    fn render_confirmation(&self, window: &MainWindow) {
        window.set_confirm_visible(self.confirmation.is_some());
        window.set_confirm_title(
            match self.confirmation {
                Some(Confirmation::Quit) => "Quit EML Attachment Remover?",
                Some(Confirmation::Force(_)) => "Force stop this run?",
                None => "",
            }
            .into(),
        );
        window.set_confirm_action_label(
            match self.confirmation {
                Some(Confirmation::Quit) => "Quit",
                Some(Confirmation::Force(_)) => "Force Stop",
                None => "",
            }
            .into(),
        );
        window.set_confirm_text(match self.confirmation {
            Some(Confirmation::Quit) => "Any active run will stop and waiting runs will be cancelled. The app will close after processing and report export have stopped safely.",
            Some(Confirmation::Force(_)) => "Earlier results will remain. A copy being saved may already have been created.",
            None => "",
        }.into());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unavailable_report_retains_failure_without_hiding_confirmed_shutdown_activity() {
        // Construction only reads preferences; no native child, UI or preference
        // write is started by this status regression.
        let mut controller = Controller::new().unwrap();
        controller.retain_report_failure();
        let failure = controller.admission_failure.clone().unwrap();
        controller.status = "Current shutdown observation".into();
        controller.retain_report_failure();
        assert_eq!(controller.status, "Current shutdown observation");
        for (since, prefix) in [
            (Instant::now(), "Quitting:"),
            (
                Instant::now() - std::time::Duration::from_secs(3),
                "Still waiting",
            ),
        ] {
            controller.closing_since = Some(since);
            let activity = controller.current_activity();
            assert!(activity.starts_with(prefix), "{activity}");
            assert!(activity.contains(&failure));
        }
    }
}
