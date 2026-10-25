//! Platform-neutral Slint controller, structured views and native-path ingress.
#![deny(unsafe_code)]
pub mod ingress;
pub mod platform;
pub mod preferences;
mod view_model;
use eml_app::{
    QueueError, RequestSnapshot, RunId, RunQueue, WorkerCompletion, WorkerFailure, WorkerRun,
    WorkerState,
};
use eml_core::{
    RunMode,
    vocabulary::{self, CountKind, RunStatus},
};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::{
    cell::RefCell,
    io,
    path::{Path, PathBuf},
    rc::Rc,
    thread::JoinHandle,
};
#[expect(
    missing_docs,
    unreachable_pub,
    reason = "Slint emits undocumented public bindings and internal public helpers"
)]
mod ui {
    slint::include_modules!();
}
pub use ui::*;
struct Entry {
    id: RunId,
    request: RequestSnapshot,
    report: Option<eml_app::ReportHandle>,
    completion: Option<WorkerCompletion>,
    launch_error: Option<String>,
}
struct Active {
    id: RunId,
    worker: WorkerRun,
    stop_requested: bool,
    force_requested: bool,
}
struct Session {
    queue: RunQueue,
    entries: Vec<Entry>,
    active: Option<Active>,
    export: Option<JoinHandle<io::Result<()>>>,
    selected: usize,
    selected_item: usize,
    mode: RunMode,
    preferences: preferences::Preferences,
    quit_when_idle: bool,
    force_quit: bool,
    should_exit: bool,
}
// Accepted records append; unchanged facts do not rebuild the Slint models.
#[derive(Clone, Copy, PartialEq, Eq)]
struct PresentationRevision {
    run: RunId,
    item: usize,
    started: bool,
    items: usize,
    summary: bool,
    failure: Option<eml_app::ProtocolFailure>,
}
/// GUI owner of queue presentation, child polling and explicit user actions.
pub struct DesktopController {
    last_view: RefCell<Option<PresentationRevision>>,
    ui: MainWindow,
    about: AboutWindow,
    session: RefCell<Session>,
    drops: RefCell<ingress::DropBatch>,
    cli: PathBuf,
    home: PathBuf,
}
fn model<T: Clone + 'static>(values: Vec<T>) -> ModelRc<T> {
    Rc::new(VecModel::from(values)).into()
}
impl DesktopController {
    /// Create the real UI around the bundled CLI and destination settings.
    pub fn new(
        cli: PathBuf,
        home: PathBuf,
        preferences: preferences::Preferences,
    ) -> Result<Rc<Self>, slint::PlatformError> {
        let ui = MainWindow::new()?;
        let about = AboutWindow::new()?;
        ui.set_product_name(eml_core::product::DISPLAY_NAME.into());
        about.set_product_name(eml_core::product::DISPLAY_NAME.into());
        ui.set_version(eml_core::product::VERSION_WITH_BUILD.as_str().into());
        about.set_version(eml_core::product::VERSION_WITH_BUILD.as_str().into());
        ui.set_destination_scope(vocabulary::DESTINATION_SCOPE.into());
        ui.set_force_notice(vocabulary::FORCE_STOP_NOTICE.into());
        let controller = Rc::new(Self {
            last_view: RefCell::new(None),
            ui,
            about,
            cli,
            home,
            drops: RefCell::default(),
            session: RefCell::new(Session {
                queue: RunQueue::default(),
                entries: Vec::new(),
                active: None,
                export: None,
                selected: 0,
                selected_item: 0,
                mode: RunMode::Create,
                preferences,
                quit_when_idle: false,
                force_quit: false,
                should_exit: false,
            }),
        });
        controller.bind();
        if controller.session.borrow().preferences.is_corrupt() {
            controller.message("Saved destination settings are corrupt and were preserved; using the default destination.");
        }
        controller.refresh();
        Ok(controller)
    }
    fn bind(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.ui.on_mode_changed(move |check| {
            if let Some(c) = weak.upgrade() {
                c.session.borrow_mut().mode = if check {
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
                if let Ok(index) = usize::try_from(index) {
                    if index < c.session.borrow().entries.len() {
                        let mut s = c.session.borrow_mut();
                        s.selected = index;
                        s.selected_item = 0;
                    }
                }
                c.refresh();
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.on_select_item(move |index| {
            if let Some(c) = weak.upgrade() {
                if let Ok(index) = usize::try_from(index) {
                    c.session.borrow_mut().selected_item = index;
                }
                c.refresh();
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.on_remove_waiting(move |index| {
            if let Some(c) = weak.upgrade() {
                let id = usize::try_from(index).ok().and_then(|index| {
                    c.session
                        .borrow()
                        .queue
                        .waiting_requests()
                        .nth(index)
                        .map(|(id, _)| id)
                });
                if let Some(id) = id {
                    c.session.borrow_mut().queue.remove_waiting(id);
                }
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
                c.ui.set_confirmation(Confirmation::ForceStop);
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.on_confirm_force_stop(move || {
            if let Some(c) = weak.upgrade() {
                c.ui.set_confirmation(Confirmation::None);
                c.force_stop();
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.on_create_copies(move || {
            if let Some(c) = weak.upgrade() {
                c.create_copies();
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
                if let Err(error) = c.about.show() {
                    c.message(&error.to_string());
                }
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.on_keep_running(move || {
            if let Some(c) = weak.upgrade() {
                c.ui.set_confirmation(Confirmation::None);
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.on_stop_and_quit(move || {
            if let Some(c) = weak.upgrade() {
                {
                    let mut s = c.session.borrow_mut();
                    s.quit_when_idle = true;
                    s.queue.cancel_waiting();
                }
                c.ui.set_confirmation(Confirmation::None);
                c.stop();
                c.tick();
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.on_force_and_quit(move || {
            if let Some(c) = weak.upgrade() {
                {
                    let mut s = c.session.borrow_mut();
                    s.force_quit = true;
                    s.queue.cancel_waiting();
                }
                c.ui.set_confirmation(Confirmation::None);
                c.force_stop();
                c.tick();
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.window().on_close_requested(move || {
            if let Some(c) = weak.upgrade() {
                if !c.request_quit() {
                    return slint::CloseRequestResponse::KeepWindowShown;
                }
            }
            slint::CloseRequestResponse::HideWindow
        });
    }
    /// The main component for native dialog parenting and toolkit-boundary tests.
    pub fn window(&self) -> MainWindow {
        self.ui.clone_strong()
    }
    /// About content includes the actual AboutSlint attribution widget.
    pub fn about_window(&self) -> AboutWindow {
        self.about.clone_strong()
    }
    /// Apply a future-request destination; existing queue snapshots stay unchanged.
    pub fn set_destination(&self, destination: Option<PathBuf>) {
        if let Err(error) = self
            .session
            .borrow_mut()
            .preferences
            .set_destination(destination)
        {
            self.message(&error.to_string());
        }
        self.refresh();
    }
    /// Enqueue one complete selection in the currently selected processing mode.
    pub fn enqueue(&self, paths: Vec<PathBuf>) -> Result<RunId, QueueError> {
        let id = {
            let mut s = self.session.borrow_mut();
            let request = RequestSnapshot::new(
                paths,
                s.preferences.destination().map(Path::to_owned),
                s.mode,
            )?;
            s.queue.enqueue(request)?
        };
        self.start_next();
        self.refresh();
        Ok(id)
    }
    /// Add one window-dropped path; a zero-delay timer commits the whole native drop.
    pub fn deliver_drop(self: &Rc<Self>, path: PathBuf) {
        if self.drops.borrow_mut().push(path) {
            let weak = Rc::downgrade(self);
            slint::Timer::single_shot(std::time::Duration::ZERO, move || {
                if let Some(c) = weak.upgrade() {
                    let paths = c.drops.borrow_mut().commit();
                    if let Some(paths) = paths {
                        if let Err(error) = c.enqueue(paths) {
                            c.message(&error.to_string());
                        }
                    }
                }
            });
        }
    }
    /// Cancel an uncommitted hover/drop batch.
    pub fn cancel_drop(&self) {
        self.drops.borrow_mut().cancel();
    }
    fn start_next(&self) {
        let mut s = self.session.borrow_mut();
        if s.quit_when_idle
            || s.force_quit
            || s.should_exit
            || self.ui.get_confirmation() == Confirmation::Quit
        {
            return;
        }
        let next = match s.queue.start_next() {
            Ok(value) => value,
            Err(QueueError::ReportSpace) => return,
            Err(error) => {
                drop(s);
                self.message(&error.to_string());
                return;
            }
        };
        let Some((id, request)) = next else {
            return;
        };
        let mut entry = Entry {
            id,
            request,
            report: None,
            completion: None,
            launch_error: None,
        };
        match WorkerRun::start(&self.cli, &entry.request, &self.home) {
            Ok(worker) => {
                entry.report = Some(worker.report().clone());
                s.active = Some(Active {
                    id,
                    worker,
                    stop_requested: false,
                    force_requested: false,
                });
            }
            Err(error) => {
                entry.launch_error = Some(error.to_string());
                let _ = s.queue.complete(id);
            }
        }
        s.entries.push(entry);
        s.selected = s.entries.len() - 1;
        s.selected_item = 0;
        platform::set_busy(s.active.is_some() || s.export.is_some());
    }
    /// Poll actual child/pipe and export settlement without blocking the event loop.
    pub fn tick(&self) {
        if platform::take_quit() {
            self.request_quit();
        }
        let mut messages = Vec::new();
        let refresh_needed;
        {
            let mut s = self.session.borrow_mut();
            refresh_needed = s.active.is_some() || s.export.is_some();
            let completed = if let Some(active) = &mut s.active {
                match active.worker.poll() {
                    Ok(completion) => completion.map(|c| (active.id, c)),
                    Err(error) => {
                        messages.push(error.to_string());
                        None
                    }
                }
            } else {
                None
            };
            if let Some((id, completion)) = completed {
                if self.ui.get_confirmation() == Confirmation::ForceStop {
                    self.ui.set_confirmation(Confirmation::None);
                }

                if completion.failure == Some(WorkerFailure::Identity) {
                    messages.push(vocabulary::WORKER_IDENTITY_HINT.into());
                }
                if matches!(
                    completion.state,
                    WorkerState::Incomplete | WorkerState::ForceStopped
                ) {
                    messages.push(vocabulary::INCOMPLETE_REPORT_NOTICE.into());
                }
                if completion.state == WorkerState::ForceStopped {
                    messages.push(vocabulary::FORCE_STOP_NOTICE.into());
                }
                if !completion.stderr.is_empty() {
                    messages.extend(
                        String::from_utf8_lossy(&completion.stderr)
                            .lines()
                            .map(str::to_owned),
                    );
                }

                if let Some(entry) = s.entries.iter_mut().find(|entry| entry.id == id) {
                    entry.completion = Some(completion);
                }
                let _ = s.queue.complete(id);
                s.active = None;
            }
            if s.export.as_ref().is_some_and(|job| job.is_finished()) {
                let result = s.export.take().expect("finished export").join();
                messages.push(match result {
                    Ok(Ok(())) => vocabulary::REPORT_SAVED.into(),
                    Ok(Err(error)) if error.kind() == io::ErrorKind::AlreadyExists => {
                        vocabulary::NO_REPLACEMENT.into()
                    }
                    Ok(Err(error)) => error.to_string(),
                    Err(_) => vocabulary::REPORT_EXPORT_FAILED.into(),
                });
            }

            if self.ui.get_confirmation() == Confirmation::Quit
                && s.active.is_none()
                && s.export.is_none()
            {
                self.ui.set_confirmation(Confirmation::None);
                s.should_exit = true;
            }
            if (s.quit_when_idle && s.active.is_none() && s.export.is_none())
                || (s.force_quit && s.active.is_none())
            {
                s.should_exit = true;
            }
            platform::set_busy(s.active.is_some() || s.export.is_some());
        }
        if !messages.is_empty() {
            self.ui.set_messages(model(
                messages
                    .iter()
                    .flat_map(|message| message.lines())
                    .map(|line| DetailView {
                        text: eml_protocol::DisplayText::new(line).to_string().into(),
                    })
                    .collect(),
            ));
        }
        self.start_next();
        if refresh_needed {
            self.refresh();
        }
    }
    /// Whether Quit must wait for a running process or report export.
    pub fn is_busy(&self) -> bool {
        let s = self.session.borrow();
        s.active.is_some() || s.export.is_some()
    }
    /// Return true for idle Quit, otherwise show an in-app confirmation.
    pub fn request_quit(&self) -> bool {
        if self.is_busy() {
            self.ui.set_confirmation(Confirmation::Quit);
            false
        } else {
            self.session.borrow_mut().should_exit = true;
            true
        }
    }
    /// Whether the outer event loop should terminate after owned child settlement.
    pub fn should_exit(&self) -> bool {
        self.session.borrow().should_exit
    }
    /// Request a single cooperative Stop of the active run.
    pub fn stop(&self) {
        let result = {
            let mut s = self.session.borrow_mut();
            s.active.as_mut().map(|active| {
                active.stop_requested = true;
                active.worker.stop()
            })
        };
        if let Some(Err(error)) = result {
            self.message(&error.to_string());
        }
        self.refresh();
    }
    /// Force Stop after the user's explicit confirmation.
    pub fn force_stop(&self) {
        let result = {
            let mut s = self.session.borrow_mut();
            s.active.as_mut().map(|active| {
                active.force_requested = true;
                active.worker.force_stop()
            })
        };
        if let Some(Err(error)) = result {
            self.message(&error.to_string());
        }
        self.refresh();
    }
    /// Start a new Create request on the selected Check's paths, repeating admission.
    pub fn create_copies(&self) {
        let paths = {
            let s = self.session.borrow();
            s.entries
                .get(s.selected)
                .filter(|entry| can_create(entry))
                .map(|entry| entry.request.request().paths.clone())
        };
        if let Some(paths) = paths {
            self.session.borrow_mut().mode = RunMode::Create;
            self.ui.set_check_only(false);
            if let Err(error) = self.enqueue(paths) {
                self.message(&error.to_string());
            }
        }
    }
    /// Save the selected settled stream in a background thread, never replacing a file.
    pub fn start_export(&self, path: PathBuf) {
        let jsonl = {
            let s = self.session.borrow();
            if s.export.is_some() {
                return;
            }
            s.entries
                .get(s.selected)
                .and_then(|entry| entry.completion.as_ref())
                .map(|completion| completion.jsonl.clone())
        };
        let Some(jsonl) = jsonl else {
            return;
        };
        match std::thread::Builder::new()
            .name("report-export".into())
            .spawn(move || eml_app::export_report(&path, &jsonl))
        {
            Ok(job) => {
                self.session.borrow_mut().export = Some(job);
                platform::set_busy(true);
            }
            Err(error) => self.message(&error.to_string()),
        }
        self.refresh();
    }
    /// Native destination from an earlier qualified copy receipt, for advisory reveal.
    pub fn selected_copy(&self) -> Option<PathBuf> {
        let s = self.session.borrow();
        let entry = s.entries.get(s.selected)?;
        entry
            .report
            .as_ref()?
            .with_report(|report| report.reveal_path(s.selected_item))
            .flatten()
    }
    /// Render a diagnostic as separate, sanitized text elements.
    pub fn message(&self, message: &str) {
        self.ui.set_messages(model(
            message
                .lines()
                .map(|line| DetailView {
                    text: eml_protocol::DisplayText::new(line).to_string().into(),
                })
                .collect(),
        ));
    }
    fn dismiss(&self) {
        {
            let mut s = self.session.borrow_mut();
            let selected = s.selected;
            if let Some(entry) = s.entries.get(selected) {
                let id = entry.id;
                if s.queue.dismiss(id) {
                    s.entries.remove(selected);
                    s.selected = selected.min(s.entries.len().saturating_sub(1));
                    s.selected_item = 0;
                }
            }
        }
        self.start_next();
        self.refresh();
    }
    fn refresh(&self) {
        let s = self.session.borrow();
        self.ui
            .set_queue_notice(if s.queue.is_paused_for_report_space() {
                vocabulary::report_space_notice(eml_app::MAX_RETAINED_REPORTS).into()
            } else {
                "".into()
            });
        self.ui
            .set_mode_label(vocabulary::mode_label(s.mode).into());
        self.ui.set_destination_label(
            s.preferences
                .destination()
                .map(|p| eml_protocol::TerminalPath::new(p).to_string())
                .unwrap_or_else(|| "Beside originals".into())
                .into(),
        );
        self.ui.set_runs(model(
            s.entries
                .iter()
                .map(|entry| {
                    run_view(
                        entry.id,
                        &entry.request,
                        status(entry, s.active.as_ref()).label(),
                    )
                })
                .collect(),
        ));
        self.ui.set_waiting(model(
            s.queue
                .waiting_requests()
                .map(|(id, request)| run_view(id, request, RunStatus::Queued.label()))
                .collect(),
        ));
        let Some(entry) = s.entries.get(s.selected) else {
            self.clear_report();
            return;
        };
        self.ui.set_run_label(format!("Run {}", entry.id).into());
        self.ui
            .set_run_status(status(entry, s.active.as_ref()).label().into());
        self.ui.set_running(
            s.active
                .as_ref()
                .is_some_and(|active| active.id == entry.id),
        );
        self.ui
            .set_can_export(entry.completion.is_some() && s.export.is_none());
        self.ui
            .set_can_dismiss(entry.completion.is_some() || entry.launch_error.is_some());
        self.ui.set_can_create(can_create(entry));
        if let Some(error) = &entry.launch_error {
            self.message(error);
        }
        if self
            .last_view
            .borrow()
            .is_some_and(|revision| revision.run != entry.id || revision.item != s.selected_item)
        {
            self.ui.set_details_scroll_y(0.0);
            self.clear_items();
        }
        let Some(handle) = &entry.report else {
            self.clear_items();
            return;
        };
        handle.with_report(|report| {
            let revision = PresentationRevision {
                run: entry.id,
                item: s.selected_item,
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
                .set_items(model(report.items().iter().map(view_model::item).collect()));
            self.ui.set_counts(model(view_model::counts(report)));
            self.ui
                .set_can_reveal(report.reveal_path(s.selected_item).is_some());
            let Some(item) = report.items().get(s.selected_item) else {
                self.ui.set_has_item(false);
                return;
            };
            let view = view_model::item(item);
            self.ui.set_has_item(true);
            self.ui.set_item_source(view.source);
            self.ui.set_item_destination(view.destination);
            self.ui.set_item_status(view.status);
            let (details, parts, fields) = view_model::details(item);
            self.ui.set_details(model(details));
            self.ui
                .set_byte_counts(model(view_model::byte_counts(item)));
            self.ui.set_removal_heading(parts.into());
            self.ui.set_field_heading(fields.into());
            self.ui.set_removals(model(view_model::removals(item)));
            self.ui.set_stripped(model(
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
        self.ui.set_items(model(Vec::new()));
        self.ui.set_counts(model(Vec::new()));
        self.ui.set_has_item(false);
        self.ui.set_details(model(Vec::new()));
        self.ui.set_removals(model(Vec::new()));
        self.ui.set_stripped(model(Vec::new()));
        self.ui.set_byte_counts(model(Vec::new()));
        *self.last_view.borrow_mut() = None;
        self.ui.set_can_reveal(false);
    }
    fn clear_report(&self) {
        self.clear_items();
        self.ui
            .set_run_label("Drop messages or choose files to begin".into());
        self.ui.set_run_status("".into());
        self.ui.set_running(false);
        self.ui.set_can_export(false);
        self.ui.set_can_dismiss(false);
        self.ui.set_can_create(false);
    }
}
fn run_view(id: RunId, request: &RequestSnapshot, status: &str) -> RunView {
    RunView {
        label: format!("Run {id}").into(),
        status: status.into(),
        mode: vocabulary::mode_label(request.request().mode).into(),
        selection: request
            .request()
            .paths
            .first()
            .map(|p| eml_protocol::TerminalPath::new(p).to_string())
            .unwrap_or_default()
            .into(),
        selections: view_model::count(CountKind::Selections, request.request().paths.len() as u32),
    }
}
fn status(entry: &Entry, active: Option<&Active>) -> RunStatus {
    if entry.launch_error.is_some() {
        return RunStatus::StartFailed;
    }
    if let Some(completion) = &entry.completion {
        if completion.failure == Some(WorkerFailure::Identity) {
            return RunStatus::IdentityFailure;
        }
        return match completion.state {
            WorkerState::Completed => RunStatus::Completed,
            WorkerState::CompletedWithFailures => RunStatus::CompletedWithFailures,
            WorkerState::Stopped => RunStatus::Stopped,
            WorkerState::StoppedBeforeStart => RunStatus::StoppedBeforeStart,
            WorkerState::ForceStopped => RunStatus::ForceStopped,
            WorkerState::RequestRefused => RunStatus::RequestRefused,
            WorkerState::Incomplete => RunStatus::Incomplete,
        };
    }
    if let Some(active) = active.filter(|active| active.id == entry.id) {
        if active.force_requested {
            return RunStatus::ForceStopRequested;
        }
        if active.stop_requested {
            return RunStatus::StopRequested;
        }
        return if entry.request.request().mode == RunMode::DryRun {
            RunStatus::Checking
        } else {
            RunStatus::Creating
        };
    }
    RunStatus::Queued
}

fn can_create(entry: &Entry) -> bool {
    entry.request.request().mode == RunMode::DryRun
        && entry.completion.as_ref().is_some_and(|completion| {
            completion.failure.is_none()
                && matches!(
                    completion.state,
                    WorkerState::Completed | WorkerState::CompletedWithFailures
                )
        })
}
