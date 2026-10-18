//! View actions over the reusable session; no message or publication policy.
use crate::{
    native::{DialogResult, NativeDialog, OpenKind},
    preferences::DestinationPreference,
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
fn display_lines(text: &str) -> String {
    text.split('\n')
        .map(|line| DisplayText::new(line).to_string())
        .collect::<Vec<_>>()
        .join("\n")
}
fn terminal_label(completion: &eml_app::WorkerCompletion) -> &'static str {
    use eml_app::WorkerFailure;
    match completion.failure {
        Some(WorkerFailure::Request) => {
            "Selection not started. Choose an existing copy destination or Beside originals. Originals are unchanged."
        }
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
        ItemStatus::Created => format!(
            "Created copy · Removed {} {}",
            item.removed.len(),
            if item.removed.len() == 1 {
                "part"
            } else {
                "parts"
            }
        ),
        ItemStatus::UpToDate => "Already up to date".into(),
        ItemStatus::Planned => "Planned".into(),
        ItemStatus::Failed => match item.publication.state {
            Publication::Published => "Copy published with an error".into(),
            Publication::Unknown => "Publication unconfirmed".into(),
            Publication::NotPublished if item.existing_observation.value().is_some() => {
                "Existing copy could not be verified".into()
            }
            Publication::NotPublished => "Could not create copy".into(),
        },
    }
}
fn report_artwork(
    facts: &eml_app::RunReport,
    completion: Option<&eml_app::WorkerCompletion>,
) -> crate::view::ArtworkState {
    use crate::view::ArtworkState;
    let Some(completion) = completion else {
        return ArtworkState::Processing;
    };
    if facts.failure().is_some() || completion.failure.is_some() {
        return ArtworkState::Failure;
    }
    if facts.summary().is_some_and(|summary| summary.interrupted)
        || completion.signal.is_some()
        || completion.exit_code == Some(130)
        || completion.exit_code.is_none()
    {
        return ArtworkState::Stopped;
    }
    if completion.exit_code != Some(0)
        || facts.summary().is_some_and(|summary| {
            summary.counts.failed > 0
                || summary.run_error.value().is_some()
                || !summary.collection_complete
        })
    {
        return if facts
            .summary()
            .is_some_and(|summary| summary.counts.created > 0 || summary.counts.up_to_date > 0)
        {
            ArtworkState::Attention
        } else {
            ArtworkState::Failure
        };
    }
    ArtworkState::Success
}
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
struct RevealTask {
    path: PathBuf,
    actor: JoinHandle<std::io::Result<bool>>,
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
    dialog: Option<Dialog>,
    confirmation: Option<Confirmation>,
    export: Option<ExportTask>,
    reveal: Option<RevealTask>,
    closing_since: Option<Instant>,
    admission_failure: Option<String>,
    notice: Option<String>,
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
    pub(crate) fn system_notice(&mut self, message: &str) {
        self.notice
            .get_or_insert_with(|| message.chars().take(1000).collect());
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
            Err(error) => self.status = error.to_string(),
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
                        let confirmed = Instant::now();
                        self.closing_since = Some(confirmed);
                        self.invalidate_dialog();
                        if let Some(export) = &self.export {
                            export.control.request_stop();
                        }
                        if let Err(error) = self.session.confirm_shutdown(confirmed) {
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
            Action::Reveal if self.reveal.is_none() => {
                let captured = self
                    .session
                    .reports()
                    .iter()
                    .find(|report| Some(report.id()) == self.selected)
                    .and_then(|report| {
                        report
                            .report()
                            .inspect(|facts| facts.reveal_path(self.item))
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
                        Ok(actor) => self.reveal = Some(RevealTask { path, actor }),
                        Err(error) => self
                            .system_notice(&format!("Could not start Finder navigation: {error}")),
                    }
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
                                accept_result: true,
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
            Err(error) => self.status = format!("Could not start report export: {error}"),
        }
    }
    /// Applies bounded view models before acknowledging a terminal run. This
    /// acknowledgement makes no claim about physical frame presentation.
    pub(crate) fn tick(&mut self, window: &MainWindow) -> bool {
        if self
            .reveal
            .as_ref()
            .is_some_and(|task| task.actor.is_finished())
        {
            let task = self.reveal.take().expect("finished owned Finder task");
            match task.actor.join() {
                Ok(Ok(true)) if self.closing_since.is_none()=>{
                    if let Err(error)=crate::native::reveal(&task.path) {self.system_notice(&format!("Finder navigation: {error}"));}
                }
                Ok(Ok(true))=>{},
                Ok(Ok(false))=>self.system_notice("The earlier reported copy is unavailable at that address. Its receipt remains unchanged; Finder navigation does not reverify it."),
                Ok(Err(error))=>self.system_notice(&format!("Finder navigation is unavailable: {error}. The earlier receipt remains unchanged.")),
                Err(_)=>self.boundary_failure("Finder navigation task failed; its owned I/O settlement could not be verified."),
            }
        }
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
                Ok(outcome) => self.system_notice(&format!("Report export: publication {:?}; address {:?}; final verification {:?}; file sync {:?}; namespace sync {:?}; cleanup {:?}. {}", outcome.publication, outcome.address_verified, outcome.final_file_verified, outcome.file_sync, outcome.namespace_sync, outcome.cleanup, outcome.failures.join("; "))),
                Err(_) => self.boundary_failure("Report export panicked; its filesystem outcome is unverified. Inspect the chosen destination before trying again."),
            }
            // The extra reservation is released only after task join and after
            // the task's serialized buffer has been destroyed.
            drop(export.reservation);
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
        if self.closing_since.is_none()
            && self.admission_failure.is_none()
            && !self.session.blocked()
        {
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
        self.closing_since.is_some()
            && self.session.worker_shutdown_settled()
            && self.dialog.is_none()
            && self.export.is_none()
            && self.reveal.is_none()
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
                    || {
                        let mut text = format!("{} received", facts.items().len());
                        if let Some(missing) =
                            facts.unreported_item_count().filter(|count| *count > 0)
                        {
                            text.push_str(&format!("\n{missing} without reported result"));
                        }
                        text
                    },
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
                let destination: String = match &request.request().output {
                    eml_core::OutputLocation::BesideSource => "Beside originals".into(),
                    eml_core::OutputLocation::Directory(path) => {
                        DisplayText::new(&path.to_string_lossy()).to_string()
                    }
                };
                format!(
                    "Run {id}: Not started\n{} selections\nSave copies: {destination}",
                    request.request().paths.len(),
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
        let mut destination = self.preference.destination().map_or_else(
            |error| error.to_string(),
            |path| {
                path.map_or_else(
                    || "Beside each original".into(),
                    |path| DisplayText::new(&path.to_string_lossy()).to_string(),
                )
            },
        );
        if self.preference.session_only() {
            destination.push_str(" (this session only)");
        }
        window.set_destination(destination.into());
        let activity = if let Some(failure) = &self.admission_failure {
            if failure == &self.status {
                failure.clone()
            } else {
                format!("{failure}\n{}", self.status)
            }
        } else if self
            .closing_since
            .is_some_and(|start| start.elapsed() >= std::time::Duration::from_secs(2))
        {
            "Still waiting for owned worker, dialog or native I/O. The app will exit only after actual settlement.".into()
        } else if self.session.blocked() {
            format!(
                "Further processing is closed because worker lifecycle could not be verified. Retained reports remain available.\n{}",
                self.status
            )
        } else {
            self.status.clone()
        };
        window.set_activity(display_lines(&activity).into());
        window.set_notice_text(
            DisplayText::new(self.notice.as_deref().unwrap_or(""))
                .to_string()
                .into(),
        );
        window.set_stop_available(
            self.closing_since.is_none()
                && self.session.control().is_some_and(|control| {
                    control
                        .intent(Instant::now())
                        .is_ok_and(|intent| intent.interrupts == 0 && !intent.reaped)
                }),
        );
        window.set_force_available(
            self.closing_since.is_none()
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
        window.set_reveal_available(
            self.closing_since.is_none()
                && idle_dialog
                && self.reveal.is_none()
                && selected.is_some_and(|report| {
                    report
                        .report()
                        .inspect(|facts| facts.reveal_path(self.item).is_some())
                        .ok()
                        .flatten()
                        .unwrap_or(false)
                }),
        );
        window.set_report_selected(
            self.closing_since.is_none()
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
                if let Some(missing) = facts.unreported_item_count().filter(|count| *count > 0) {
                    summary.push_str(&format!("\n{missing} {} no reported result. Complete copies may exist; inspect the destination.", if missing == 1 {"item has"} else {"items have"}));
                }
                if let Some(completion) = completion {summary.push_str(&format!("\n{}", terminal_label(completion)));}
                summary.push_str(&format!("\nItems page {} of {pages}", self.page + 1));
                let rows = facts.items().iter().skip(self.page * 64).take(64).map(|item| {
                    let basename = item.source.display.rsplit('/').next().unwrap_or("Message"); let name: String = DisplayText::new(basename).to_string().chars().take(180).collect();
                    format!("{} · {}", name, item_label(item)).into()
                }).collect::<Vec<slint::SharedString>>();
                let mut detail = String::new();
                let mut technical = format!("Protocol failure: {:?}\n", facts.failure());
                if let Some(completion) = completion {technical.push_str(&format!("Worker exit {:?}; signal {:?}; failure {:?}; settled {}; reusable {}\nInterrupt calls {}; first interrupt observed {}\n{}\nStderr{}:\n{}\n", completion.exit_code, completion.signal, completion.failure, completion.settled, completion.reusable, completion.interrupt_calls_succeeded, completion.first_interrupt_observed, DisplayText::new(&completion.faults.join("; ")), if completion.stderr_truncated {" (truncated)"} else {""}, display_lines(&String::from_utf8_lossy(&completion.stderr))));}
                if let Some(item) = facts.items().get(self.item) {
                    detail.push_str(&format!("{}\nSource: {}\nCopy: {}\n", item_label(item), DisplayText::new(&item.source.display), item.destination.value().map_or_else(|| "—".into(), |path| DisplayText::new(&path.display).to_string())));
                    if let Some(saved) = facts.saved_bytes(self.item) {detail.push_str(&format!("Saved {saved} bytes\n"));}
                    if let Some(error) = item.error.value() {detail.push_str(&format!("{}\nOriginal message unchanged.\n", DisplayText::new(&error.detail)));}
                    let pages = item.removed.len().div_ceil(64).max(1);
                    self.part_page = self.part_page.min(pages - 1);
                    detail.push_str(&format!("Removed parts page {} of {pages}\n", self.part_page + 1));
                    for part in item.removed.iter().skip(self.part_page * 64).take(64) {
                        detail.push_str(&format!("{} · {} · {} bytes · {:?} · section {}\n", DisplayText::new(part.filename.value().map_or("Unnamed part", String::as_str)), DisplayText::new(&part.content_type), part.bytes, part.reason, DisplayText::new(&part.section)));
                    }
                    if !item.stripped_fields.is_empty() {detail.push_str(&format!("Removed stale fields: {}\n", DisplayText::new(&item.stripped_fields.join(", "))));}
                    technical.push_str(&format!("Publication: {:#?}\nExisting copy: {:#?}\nItem error: {:#?}\n", item.publication, item.existing_observation.value(), item.error.value()));
                }
                let artwork = report_artwork(facts, completion);
                let navigation = (
                    self.page > 0,
                    self.page + 1 < pages,
                    self.part_page > 0,
                    facts.items().get(self.item).is_some_and(|item| self.part_page + 1 < item.removed.len().div_ceil(64)),
                );
                Some((key, summary, rows, detail, technical, artwork, navigation))
            });
            match inspected {
                Ok(Some(Some((key, summary, rows, detail, technical, artwork, navigation)))) => {
                    // Native/UI calls occur only after releasing the model lock.
                    self.view_key = Some(key);
                    window.set_report_text(summary.into());
                    window.set_items(ModelRc::new(VecModel::from(rows)));
                    window.set_item_text(detail.into());
                    window.set_technical_text(technical.into());
                    window.set_artwork_state(artwork);
                    window.set_previous_items_available(navigation.0);
                    window.set_next_items_available(navigation.1);
                    window.set_previous_parts_available(navigation.2);
                    window.set_next_parts_available(navigation.3);
                }
                Ok(Some(None)) => {}
                _ => {
                    if self.session.active() == Some(report.id()) {
                        applied = false;
                    }
                }
            }
        } else {
            window.set_previous_items_available(false);
            window.set_next_items_available(false);
            window.set_previous_parts_available(false);
            window.set_next_parts_available(false);
            self.view_key = None;
            window.set_report_text("".into());
            window.set_item_text("".into());
            window.set_technical_text("".into());
            window.set_items(ModelRc::new(VecModel::from(
                Vec::<slint::SharedString>::new(),
            )));
            window.set_artwork_state(crate::view::ArtworkState::Identity);
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
    fn run_errors_without_failed_items_are_not_presented_as_success() {
        use crate::view::ArtworkState;
        for (fixture, exit, expected) in [
            ("created", 0, ArtworkState::Success),
            ("no-inputs", 1, ArtworkState::Failure),
            ("collection-limit", 1, ArtworkState::Failure),
            ("started-preedge-cancellation", 130, ArtworkState::Stopped),
        ] {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(format!("../../tests/contract/protocol/{fixture}.jsonl"));
            let bytes = std::fs::read_to_string(path)
                .unwrap()
                .replace("@VERSION@", "test-version")
                .into_bytes();
            let mut facts = eml_app::RunReport::new("test-version");
            let mut splitter = eml_app::FrameSplitter::default();
            splitter
                .append(&bytes, |frame| facts.accept_frame(frame))
                .unwrap();
            facts.finish(Some(exit), splitter.finish()).unwrap();
            let completion = eml_app::WorkerCompletion {
                exit_code: Some(exit),
                signal: None,
                input_may_have_been_sent: true,
                settled: true,
                reusable: true,
                failure: None,
                faults: Vec::new(),
                stderr: Vec::new(),
                stderr_truncated: false,
                interrupt_calls_succeeded: 0,
                first_interrupt_observed: false,
            };
            assert_eq!(
                report_artwork(&facts, Some(&completion)),
                expected,
                "{fixture}"
            );
        }
    }
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
