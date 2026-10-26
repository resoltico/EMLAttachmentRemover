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
use slint::{ComponentHandle, Model, ModelRc, VecModel};
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
    worker_error: Option<String>,
}
struct Active {
    id: RunId,
    worker: WorkerRun,
    stop_requested: bool,
    force_requested: bool,
}
struct ExportJob {
    run: RunId,
    task: JoinHandle<io::Result<()>>,
}
/// Exact settled report retained while a native Save dialog is open.
pub struct ReportExport {
    run: RunId,
    jsonl: std::sync::Arc<[u8]>,
}
impl ReportExport {
    /// Captured run identity for the suggested filename and completion feedback.
    pub fn run_id(&self) -> RunId {
        self.run
    }
}
struct Session {
    selection: Vec<PathBuf>,
    reviewing: bool,
    follow_run: Option<RunId>,
    queue: RunQueue,
    entries: Vec<Entry>,
    active: Option<Active>,
    export: Option<ExportJob>,
    selected: usize,
    selected_item: usize,
    mode: RunMode,
    preferences: preferences::Preferences,
    quit_when_idle: bool,
    force_quit: bool,
    should_exit: bool,
}
impl Session {
    fn is_closing(&self) -> bool {
        self.quit_when_idle || self.force_quit || self.should_exit
    }
    fn needs_quit_confirmation(&self) -> bool {
        self.active.is_some() || self.export.is_some() || self.queue.waiting() > 0
    }
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
    licenses: LicenseWindow,
    session: RefCell<Session>,
    drops: RefCell<ingress::DropBatch>,
    cli: PathBuf,
    home: PathBuf,
}
// Update data in place: polling must not discard row controls, keyboard focus,
// or accessibility nodes when accepted facts are unchanged or append-only.
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
    /// Create the real UI around the bundled CLI and destination settings.
    pub fn new(
        cli: PathBuf,
        home: PathBuf,
        preferences: preferences::Preferences,
    ) -> Result<Rc<Self>, slint::PlatformError> {
        let ui = MainWindow::new()?;
        let about = AboutWindow::new()?;
        let licenses = LicenseWindow::new()?;
        licenses.set_licenses(include_str!("../../../LICENSE").into());
        about.set_copyright(
            include_str!("../../../LICENSE")
                .lines()
                .nth(1)
                .unwrap_or_default()
                .replace("(c)", "©")
                .into(),
        );
        ui.set_product_name(eml_core::product::DISPLAY_NAME.into());
        about.set_product_name(eml_core::product::DISPLAY_NAME.into());
        ui.set_native_about(cfg!(all(feature = "desktop", target_os = "macos")));
        about.set_version(eml_core::product::VERSION_WITH_BUILD.as_str().into());
        ui.set_report_capacity(eml_app::MAX_RETAINED_REPORTS as i32);
        ui.set_force_notice(vocabulary::FORCE_STOP_NOTICE.into());
        let controller = Rc::new(Self {
            last_view: RefCell::new(None),
            ui,
            about,
            licenses,
            cli,
            home,
            drops: RefCell::default(),
            session: RefCell::new(Session {
                selection: Vec::new(),
                reviewing: false,
                follow_run: None,
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
        controller.refresh();
        Ok(controller)
    }
    fn bind(self: &Rc<Self>) {
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
                s.selection.clear();
                s.reviewing = false;
                drop(s);
                c.refresh();
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.on_review_selection(move || {
            if let Some(c) = weak.upgrade() {
                c.session.borrow_mut().reviewing = true;
                c.refresh();
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.on_remove_selection(move |index| {
            if let Some(c) = weak.upgrade() {
                let mut s = c.session.borrow_mut();
                if let Ok(index) = usize::try_from(index)
                    && index < s.selection.len()
                {
                    s.selection.remove(index);
                }
                s.reviewing = !s.selection.is_empty();
                drop(s);
                c.refresh();
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.on_mode_changed(move |check| {
            if let Some(c) = weak.upgrade() {
                c.clear_operation();
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
                if let Ok(index) = usize::try_from(index)
                    && index < c.session.borrow().entries.len()
                {
                    let mut s = c.session.borrow_mut();
                    s.selected = index;
                    s.selected_item = 0;
                    s.reviewing = false;
                    s.follow_run = None;
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
                    s.selected_item = index;
                    s.follow_run = None;
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
                if c.ui.get_confirmation() != Confirmation::None {
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
                    let source = s
                        .entries
                        .iter()
                        .find(|entry| entry.id == active.id)
                        .and_then(|entry| entry.request.request().paths.first())
                        .map(|path| eml_protocol::TerminalPath::new(path).to_string())
                        .unwrap_or_default();
                    format!(
                        "{}\n\nRun {} · {}",
                        vocabulary::FORCE_STOP_NOTICE,
                        active.id,
                        source
                    )
                };
                c.ui.set_force_notice(notice.into());
                c.ui.set_confirmation(Confirmation::ForceStop);
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.on_confirm_force_stop(move || {
            if let Some(c) = weak.upgrade() {
                if c.ui.get_confirmation() != Confirmation::ForceStop {
                    return;
                }
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
            if let Some(c) = weak.upgrade()
                && let Err(error) = c.about.show()
            {
                c.message(&error.to_string());
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
            if let Some(c) = weak.upgrade()
                && let Err(error) = c.licenses.show()
            {
                c.message(&error.to_string());
            }
        });
        let weak = Rc::downgrade(self);
        self.licenses.on_dismiss(move || {
            if let Some(c) = weak.upgrade() {
                let _ = c.licenses.hide();
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
                if c.ui.get_confirmation() != Confirmation::Quit {
                    return;
                }
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
                if c.ui.get_confirmation() != Confirmation::Quit {
                    return;
                }
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
            if let Some(c) = weak.upgrade()
                && !c.request_quit()
            {
                return slint::CloseRequestResponse::KeepWindowShown;
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
    /// Readable software/artwork terms and the packaged third-party notices action.
    pub fn license_window(&self) -> LicenseWindow {
        self.licenses.clone_strong()
    }
    /// Apply a future-request destination; existing queue snapshots stay unchanged.
    pub fn set_destination(&self, destination: Option<PathBuf>) {
        self.clear_operation();
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
    /// Review native selections without starting a worker or touching source files.
    /// Exact duplicate paths are shown once; filesystem aliases remain core-owned.
    pub fn stage(&self, paths: Vec<PathBuf>) -> Result<(), QueueError> {
        let mut s = self.session.borrow_mut();
        if s.is_closing() {
            return Err(QueueError::ShuttingDown);
        }
        let mut selection = s.selection.clone();
        for path in paths {
            if !selection.contains(&path) {
                selection.push(path);
            }
        }
        RequestSnapshot::new(
            selection.clone(),
            s.preferences.destination().map(Path::to_owned),
            s.mode,
        )?;
        s.selection = selection;
        s.reviewing = true;
        s.follow_run = None;
        drop(s);
        self.clear_operation();
        self.refresh();
        Ok(())
    }
    /// Capture the reviewed selection's current settings and explicitly submit it.
    /// Refused admission retains every staged path for correction or retry.
    pub fn start_selection(&self) -> Result<RunId, QueueError> {
        self.clear_operation();
        let id = {
            let mut s = self.session.borrow_mut();
            if s.is_closing() {
                return Err(QueueError::ShuttingDown);
            }
            let request = RequestSnapshot::new(
                s.selection.clone(),
                s.preferences.destination().map(Path::to_owned),
                s.mode,
            )?;
            let id = s.queue.enqueue(request)?;
            s.follow_run = Some(id);
            s.selection.clear();
            s.reviewing = false;
            id
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
                    if let Some(paths) = paths
                        && let Err(error) = c.stage(paths)
                    {
                        c.message(&error.to_string());
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
        if s.is_closing() || self.ui.get_confirmation() == Confirmation::Quit {
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
            worker_error: None,
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
        if s.follow_run == Some(id) || s.entries.len() == 1 {
            s.selected = s.entries.len() - 1;
            s.selected_item = 0;
            s.follow_run = None;
        }
        self.clear_operation();
        platform::set_busy(s.needs_quit_confirmation());
    }
    /// Poll actual child/pipe and export settlement without blocking the event loop.
    pub fn tick(&self) {
        platform::refresh_menu();
        if platform::take_about() {
            self.ui.invoke_about();
        }
        if platform::take_quit() {
            self.request_quit();
        }
        let refresh_needed;
        {
            let mut s = self.session.borrow_mut();
            refresh_needed = s.active.is_some() || s.export.is_some();
            let completed = if let Some(active) = &mut s.active {
                match active.worker.poll() {
                    Ok(completion) => completion.map(|c| (active.id, c)),
                    Err(error) => {
                        let id = active.id;
                        if let Some(entry) = s.entries.iter_mut().find(|e| e.id == id) {
                            entry.worker_error.get_or_insert_with(|| error.to_string());
                        }
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

                if let Some(entry) = s.entries.iter_mut().find(|entry| entry.id == id) {
                    entry.completion = Some(completion);
                }
                let _ = s.queue.complete(id);
                s.active = None;
            }
            if s.export.as_ref().is_some_and(|job| job.task.is_finished()) {
                let job = s.export.take().expect("finished export");
                let result = job.task.join();
                let message = match result {
                    Ok(Ok(())) => vocabulary::REPORT_SAVED.into(),
                    Ok(Err(error)) if error.kind() == io::ErrorKind::AlreadyExists => {
                        vocabulary::NO_REPLACEMENT.into()
                    }
                    Ok(Err(error)) => error.to_string(),
                    Err(_) => vocabulary::REPORT_EXPORT_FAILED.to_owned(),
                };
                self.operation(&format!("Export for Run {}", job.run), &message);
            }

            if self.ui.get_confirmation() == Confirmation::Quit
                && s.active.is_none()
                && s.export.is_none()
                && s.queue.waiting() == 0
            {
                self.ui.set_confirmation(Confirmation::None);
                s.should_exit = true;
            }
            if (s.quit_when_idle || s.force_quit) && s.active.is_none() && s.export.is_none() {
                s.should_exit = true;
            }
            platform::set_busy(s.needs_quit_confirmation());
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
    /// Confirm owned tasks or waiting requests; repeated committed Quit stays closed.
    pub fn request_quit(&self) -> bool {
        let s = self.session.borrow();
        if s.is_closing() {
            return false;
        }
        let needs_confirmation = s.needs_quit_confirmation();
        drop(s);
        if needs_confirmation {
            let _ = self.about.hide();
            let _ = self.licenses.hide();
            if let Err(error) = self.ui.show() {
                self.message(&error.to_string());
            }
            self.ui.window().set_minimized(false);
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
        self.clear_operation();
        let result = {
            let mut s = self.session.borrow_mut();
            s.active.as_mut().map(|active| {
                active.stop_requested = true;
                active.worker.stop()
            })
        };
        if let Some(Err(error)) = result {
            self.record_worker_error(&error);
        }
        self.refresh();
    }
    /// Force Stop after the user's explicit confirmation.
    pub fn force_stop(&self) {
        self.clear_operation();
        let result = {
            let mut s = self.session.borrow_mut();
            s.active.as_mut().map(|active| {
                active.force_requested = true;
                active.worker.force_stop()
            })
        };
        if let Some(Err(error)) = result {
            self.record_worker_error(&error);
        }
        self.refresh();
    }
    fn record_worker_error(&self, error: &io::Error) {
        let mut s = self.session.borrow_mut();
        if let Some(id) = s.active.as_ref().map(|active| active.id)
            && let Some(entry) = s.entries.iter_mut().find(|e| e.id == id)
        {
            entry.worker_error.get_or_insert_with(|| error.to_string());
        }
    }
    /// Review a fresh Create selection from a settled Check; Start repeats admission.
    pub fn create_copies(&self) {
        {
            let s = self.session.borrow();
            if s.is_closing() {
                self.message(&QueueError::ShuttingDown.to_string());
                return;
            }
            if !s.selection.is_empty() {
                self.message("A selection is already being reviewed. Finish or clear it before creating copies from this Check; the reviewed paths were preserved.");
                return;
            }
        }
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
            if let Err(error) = self.stage(paths) {
                self.message(&error.to_string());
            }
        }
    }
    /// Capture the selected settled stream before presenting a native file dialog.
    pub fn capture_export(&self) -> Option<ReportExport> {
        let mut s = self.session.borrow_mut();
        if s.export.is_some() || s.is_closing() {
            return None;
        }
        let entry = s.entries.get(s.selected)?;
        let source = ReportExport {
            run: entry.id,
            jsonl: entry.completion.as_ref()?.jsonl.clone(),
        };
        s.follow_run = None;
        Some(source)
    }
    /// Save the currently selected settled stream without a file-dialog delay.
    pub fn start_export(&self, path: PathBuf) {
        if let Some(source) = self.capture_export() {
            self.start_captured_export(source, path);
        }
    }
    /// Write the exact captured report, even if selection changed during the dialog.
    /// Existing files are never replaced.
    pub fn start_captured_export(&self, source: ReportExport, path: PathBuf) {
        self.clear_operation();
        if self.session.borrow().is_closing() {
            self.message(&QueueError::ShuttingDown.to_string());
            return;
        }
        if self.session.borrow().export.is_some() {
            self.message("A report export is already running. Wait for it to finish before saving another report.");
            return;
        }
        let ReportExport { run, jsonl } = source;
        match std::thread::Builder::new()
            .name("report-export".into())
            .spawn(move || eml_app::export_report(&path, &jsonl))
        {
            Ok(job) => {
                self.session.borrow_mut().export = Some(ExportJob { run, task: job });
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
    /// Show transient feedback for the current user operation, separate from run facts.
    pub fn message(&self, message: &str) {
        self.operation("Operation status", message);
    }
    fn operation(&self, label: &str, message: &str) {
        self.ui.set_operation_label(label.into());
        self.ui
            .set_operation_feedback(eml_protocol::DisplayText::new(message).to_string().into());
    }
    fn clear_operation(&self) {
        self.ui.set_operation_label("".into());
        self.ui.set_operation_feedback("".into());
    }
    fn dismiss(&self) {
        self.clear_operation();
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
        platform::set_busy(s.needs_quit_confirmation());
        self.ui.set_closing(s.is_closing());
        self.ui.set_check_only(s.mode == RunMode::DryRun);
        self.ui.set_selection(update_model(
            self.ui.get_selection(),
            s.selection
                .iter()
                .map(|path| view_model::path(&eml_protocol::TerminalPath::new(path).to_string()))
                .collect(),
        ));
        self.ui.set_reviewing(s.reviewing);
        self.ui.set_selected_item(s.selected_item as i32);
        self.ui.set_selected_run(s.selected as i32);
        self.ui.set_has_reports(!s.entries.is_empty());
        self.ui.set_worker_running(s.active.is_some());
        self.ui.set_show_sidebar(
            s.entries.len() > 1
                || s.queue.waiting() > 0
                || (!s.selection.is_empty() && !s.entries.is_empty()),
        );
        self.ui.set_start_label(
            if s.active.is_some() || s.entries.len() >= eml_app::MAX_RETAINED_REPORTS {
                "Queue request".into()
            } else if s.mode == RunMode::DryRun {
                "Check selection".into()
            } else {
                "Create copies".into()
            },
        );
        self.ui.set_can_start(
            !s.selection.is_empty()
                && s.queue.waiting() < eml_app::MAX_WAITING_REQUESTS
                && !s.is_closing(),
        );
        self.ui.set_start_reason(
            if s.is_closing() {
                QueueError::ShuttingDown.to_string()
            } else if s.queue.waiting() >= eml_app::MAX_WAITING_REQUESTS {
                QueueError::Full.to_string()
            } else {
                format!(
                    "{} using the reviewed paths and current destination",
                    self.ui.get_start_label()
                )
            }
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
        self.ui
            .set_queue_notice(if s.queue.is_paused_for_report_space() {
                vocabulary::report_space_notice(eml_app::MAX_RETAINED_REPORTS).into()
            } else {
                "".into()
            });
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
                    run_view(
                        entry.id,
                        &entry.request,
                        status(entry, s.active.as_ref()).label(),
                    )
                })
                .collect(),
        ));
        self.ui.set_waiting(update_model(
            self.ui.get_waiting(),
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
        self.ui
            .set_can_export(entry.completion.is_some() && s.export.is_none());
        self.ui
            .set_can_dismiss(entry.completion.is_some() || entry.launch_error.is_some());
        self.ui.set_can_create(can_create(entry));
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
            && (previous.run != entry.id || previous.item != s.selected_item)
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
                .set_work_total(report.start().map_or(0, |s| s.total as i32));
            self.ui.set_work_completed(report.items().len() as i32);

            self.ui.set_items(update_model(
                self.ui.get_items(),
                report.items().iter().map(view_model::item).collect(),
            ));
            self.ui.set_counts(update_model(
                self.ui.get_counts(),
                view_model::counts(report),
            ));
            self.ui
                .set_can_reveal(report.reveal_path(s.selected_item).is_some());
            let Some(item) = report.items().get(s.selected_item) else {
                self.ui.set_has_item(false);
                return;
            };
            let view = view_model::item(item);
            self.ui.set_has_item(true);
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
        self.ui.set_work_total(0);
        self.ui.set_work_completed(0);
        self.ui
            .set_items(update_model(self.ui.get_items(), Vec::new()));
        self.ui
            .set_counts(update_model(self.ui.get_counts(), Vec::new()));
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
    }
}
fn request_destination(request: &RequestSnapshot) -> String {
    match &request.request().output {
        eml_core::OutputLocation::BesideSource => "Beside originals".into(),
        eml_core::OutputLocation::Directory(path) => {
            eml_protocol::TerminalPath::new(path).to_string()
        }
    }
}
fn run_view(id: RunId, request: &RequestSnapshot, status: &str) -> RunView {
    RunView {
        label: format!("Run {id}").into(),
        status: status.into(),
        mode: vocabulary::mode_label(request.request().mode).into(),
        destination: request_destination(request).into(),
        selection: view_model::path(
            &request
                .request()
                .paths
                .first()
                .map(|p| eml_protocol::TerminalPath::new(p).to_string())
                .unwrap_or_default(),
        ),
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

#[cfg(test)]
mod tests;
