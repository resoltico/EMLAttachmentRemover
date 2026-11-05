//! Platform-neutral Slint controller, structured views and native-path ingress.
#![deny(unsafe_code)]
mod callbacks;
pub mod ingress;
mod navigation;
pub mod platform;
pub mod preferences;
mod present;
mod reports;
mod state;
mod view_model;
mod wording;
use eml_app::{
    QueueError, RequestSnapshot, RunId, WorkerCompletion, WorkerFailure, WorkerRun, WorkerState,
};
use eml_core::{
    RunMode,
    vocabulary::{self, CountKind, RunStatus},
};
use slint::{ComponentHandle, Model};
use std::{
    cell::{Cell, RefCell},
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
    presentation: state::RunPresentation,
    storage_error: Option<String>,
    load_error: Option<String>,
    stored: Option<eml_app::StoredReport>,
    saved_report: Option<PathBuf>,
}
enum ReportTask {
    Store(RunId, JoinHandle<io::Result<eml_app::StoredReport>>),
    Load(RunId, JoinHandle<io::Result<WorkerCompletion>>),
}
impl ReportTask {
    fn is_finished(&self) -> bool {
        match self {
            Self::Store(_, task) => task.is_finished(),
            Self::Load(_, task) => task.is_finished(),
        }
    }
}
struct Active {
    id: RunId,
    worker: WorkerRun,
    stop: state::Stop,
    estimate: state::ProgressEstimate,
}
struct ExportJob {
    run: RunId,
    path: PathBuf,
    task: JoinHandle<io::Result<()>>,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum OpenTarget {
    Main,
    #[cfg(feature = "desktop")]
    About,
    #[cfg(any(feature = "desktop", test))]
    Licenses,
}
struct OpenJob {
    target: OpenTarget,
    serial: u64,
    failure: &'static str,
    task: JoinHandle<Result<(), String>>,
}
/// Exact settled report retained while a native Save dialog is open.
#[derive(Clone)]
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
/// Opaque lifetime of one export draft's native folder request.
pub struct ExportFolderRequest {
    token: std::rc::Weak<()>,
}
struct ExportDraft {
    token: Rc<()>,
    source: ReportExport,
    directory: PathBuf,
}
struct SessionRuntime {
    state: state::Session,
    entries: Vec<Entry>,
    active: Option<Active>,
    export: Option<ExportJob>,
    preferences: preferences::Preferences,
    quit_when_idle: bool,
    force_quit: bool,
    should_exit: bool,
    report_task: Option<ReportTask>,
}
impl SessionRuntime {
    fn is_closing(&self) -> bool {
        self.quit_when_idle || self.force_quit || self.should_exit
    }
    fn memory_full(&self) -> bool {
        self.entries
            .iter()
            .filter(|entry| entry.completion.is_some())
            .count()
            >= 3
    }
    fn admission_error(&self) -> Option<QueueError> {
        if self.is_closing() {
            Some(QueueError::ShuttingDown)
        } else if self.state.confirmation.is_some() {
            Some(QueueError::DecisionPending)
        } else if self.memory_full() {
            Some(QueueError::MemoryReportsFull)
        } else if self
            .preferences
            .destination()
            .is_some_and(|path| !path.is_dir())
        {
            Some(QueueError::DestinationUnavailable)
        } else {
            self.state.queue.capacity_error()
        }
    }
    fn selected_index(&self) -> Option<usize> {
        match self.state.view {
            state::View::Run(id) => self.entries.iter().position(|entry| entry.id == id),
            state::View::Draft | state::View::Waiting(_) => None,
        }
    }
    fn selected_entry(&self) -> Option<&Entry> {
        self.selected_index()
            .and_then(|index| self.entries.get(index))
    }
    fn selected_entry_mut(&mut self) -> Option<&mut Entry> {
        let index = self.selected_index()?;
        self.entries.get_mut(index)
    }
    fn needs_quit_confirmation(&self) -> bool {
        self.active.is_some() || self.export.is_some() || self.state.queue.waiting() > 0
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
struct PresentationRevision {
    run: RunId,
    item: Option<usize>,
    filter: state::Filter,
    sort: state::Sort,
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
    export_ui: ExportWindow,
    export_draft: RefCell<Option<ExportDraft>>,
    export_directory: RefCell<PathBuf>,
    session: RefCell<SessionRuntime>,
    drops: RefCell<ingress::DropBatch>,
    cli: PathBuf,
    home: PathBuf,
    open_jobs: RefCell<Vec<OpenJob>>,
    operation_serial: Cell<u64>,
}
// Update data in place: polling must not discard row controls, keyboard focus,
// or accessibility nodes when accepted facts are unchanged or append-only.

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
        let export_ui = ExportWindow::new()?;
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
        about.set_version(eml_core::product::ABOUT_VERSION.as_str().into());
        let controller = Rc::new(Self {
            last_view: RefCell::new(None),
            export_ui,
            export_draft: RefCell::new(None),
            export_directory: RefCell::new(home.clone()),
            ui,
            about,
            licenses,
            cli,
            home,
            drops: RefCell::default(),
            open_jobs: RefCell::default(),
            operation_serial: Cell::new(0),
            session: RefCell::new(SessionRuntime {
                state: state::Session::default(),
                entries: Vec::new(),
                active: None,
                export: None,
                preferences,
                quit_when_idle: false,
                force_quit: false,
                should_exit: false,
                report_task: None,
            }),
        });
        controller.bind();
        controller.refresh();
        Ok(controller)
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
        if !self.inputs_enabled() {
            return;
        }
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
        if s.state.confirmation.is_some() {
            return Err(QueueError::DecisionPending);
        }
        // Preserve request admission's absolute-path error before limiting count.
        if paths.iter().any(|path| !path.is_absolute()) {
            return Err(QueueError::AbsoluteSelectionRequired);
        }
        let selection = {
            let mut seen = std::collections::HashSet::new();
            s.state
                .selection
                .iter()
                .chain(&paths)
                .filter(|path| seen.insert(*path))
                // Keep one excess unique path so core rejects, rather than truncates.
                .take(eml_core::MAX_INPUT_PATHS + 1)
                .cloned()
                .collect::<Vec<_>>()
        };
        RequestSnapshot::new(
            selection.clone(),
            s.preferences.destination().map(Path::to_owned),
            s.state.mode,
        )?;
        s.state.selection = selection;
        s.state.view = state::View::Draft;
        drop(s);
        self.clear_operation();
        self.maintain_reports();
        self.refresh();
        Ok(())
    }
    /// Capture the reviewed selection's current settings and explicitly submit it.
    /// Refused admission retains every staged path for correction or retry.
    pub fn start_selection(&self) -> Result<RunId, QueueError> {
        self.clear_operation();
        let id = {
            let mut s = self.session.borrow_mut();
            if let Some(error) = s.admission_error() {
                return Err(error);
            }
            let request = RequestSnapshot::new(
                s.state.selection.clone(),
                s.preferences.destination().map(Path::to_owned),
                s.state.mode,
            )?;
            let id = s.state.queue.enqueue(request)?;
            s.state.view = state::View::Waiting(id);
            s.state.selection.clear();
            id
        };
        self.start_next();
        self.refresh();
        Ok(id)
    }
    /// Add one window-dropped path; a zero-delay timer commits the whole native drop.
    pub fn deliver_drop(self: &Rc<Self>, path: PathBuf) {
        if !self.inputs_enabled() {
            return;
        }
        self.hover_drop(false);
        if self.drops.borrow_mut().push(path) {
            let weak = Rc::downgrade(self);
            slint::Timer::single_shot(std::time::Duration::ZERO, move || {
                if let Some(c) = weak.upgrade() {
                    let paths = c.drops.borrow_mut().commit();
                    if let Some(paths) = paths
                        && c.inputs_enabled()
                        && let Err(error) = c.stage(paths)
                    {
                        c.message(&wording::queue_error(&error));
                    }
                }
            });
        }
    }
    /// Cancel an uncommitted hover/drop batch.
    pub fn cancel_drop(&self) {
        self.hover_drop(false);
        self.drops.borrow_mut().cancel();
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
            refresh_needed = s.active.is_some() || s.export.is_some() || s.report_task.is_some();
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
                if matches!(&s.state.confirmation, Some(state::PendingConfirmation::ForceStop(run)) if *run == id)
                {
                    s.state.confirmation = None;
                    self.ui.set_confirmation(Confirmation::None);
                }

                let visible = s.state.view == state::View::Run(id);
                if let Some(entry) = s.entries.iter_mut().find(|entry| entry.id == id) {
                    entry.presentation.settle(visible);
                    entry.completion = Some(completion);
                }
                let _ = s.state.queue.complete(id);
                s.active = None;
            }
            let now = now_ms();
            for entry in &mut s.entries {
                reconcile_entry(entry);
            }
            if let Some(id) = s.active.as_ref().map(|active| active.id) {
                let completed = s
                    .entries
                    .iter()
                    .find(|entry| entry.id == id)
                    .map(|entry| entry.presentation.tally.processed);
                if let (Some(completed), Some(active)) = (completed, s.active.as_mut()) {
                    active.estimate.observe(now, completed);
                }
            }
            if s.export.as_ref().is_some_and(|job| job.task.is_finished()) {
                let job = s.export.take().expect("finished export");
                let result = job.task.join();
                let saved = matches!(&result, Ok(Ok(())));
                let message = match result {
                    Ok(Ok(())) => {
                        let retained = s.entries.iter_mut().find(|entry| entry.id == job.run);
                        let display = if let Some(entry) = retained {
                            entry.saved_report = Some(job.path.clone());
                            job.path.file_name().map(Path::new).unwrap_or(&job.path)
                        } else {
                            &job.path
                        };
                        format!("Report saved: {}", eml_protocol::TerminalPath::new(display))
                    }
                    Ok(Err(error)) if error.kind() == io::ErrorKind::AlreadyExists => {
                        vocabulary::NO_REPLACEMENT.into()
                    }
                    Ok(Err(error)) => error.to_string(),
                    Err(_) => vocabulary::REPORT_EXPORT_FAILED.to_owned(),
                };
                if self
                    .export_draft
                    .borrow()
                    .as_ref()
                    .is_some_and(|draft| draft.source.run == job.run)
                {
                    self.export_ui.set_busy(false);
                    if saved {
                        self.export_draft.borrow_mut().take();
                        let _ = self.export_ui.hide();
                    } else {
                        self.export_ui.set_notice(
                            format!(
                                "{message} Choose a different file name or folder, then try again."
                            )
                            .into(),
                        );
                    }
                }
                self.operation(&format!("Export for Run {}", job.run), &message);
            }

            if matches!(
                &s.state.confirmation,
                Some(state::PendingConfirmation::Quit)
            ) && s.active.is_none()
                && s.export.is_none()
                && s.state.queue.waiting() == 0
            {
                s.state.confirmation = None;
                self.ui.set_confirmation(Confirmation::None);
                s.should_exit = true;
            }
            if (s.quit_when_idle || s.force_quit) && s.active.is_none() && s.export.is_none() {
                s.should_exit = true;
            }
            platform::set_busy(s.needs_quit_confirmation());
        }
        self.maintain_reports();
        self.start_next();
        self.poll_open_jobs();
        if refresh_needed {
            self.refresh();
        }
    }
    /// Whether Quit must wait for a running process or report export.
    pub fn is_busy(&self) -> bool {
        let s = self.session.borrow();
        s.active.is_some()
            || s.export.is_some()
            || (!s.is_closing()
                && (s.report_task.is_some()
                    || (!s.memory_full()
                        && s.state.queue.stored_bytes() < eml_app::MAX_STORED_REPORT_BYTES
                        && s.state.queue.waiting() > 0)))
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
            let _ = self.export_ui.hide();
            if let Err(error) = self.ui.show() {
                self.message(&error.to_string());
            }
            self.ui.window().set_minimized(false);
            self.set_confirmation(Some(state::PendingConfirmation::Quit));
            false
        } else {
            self.cancel_export();
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
                if active.stop != state::Stop::Running {
                    return Ok(());
                }
                active.stop = state::Stop::Requested { at: now_ms() };
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
                active.stop = state::Stop::Forced;
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
    /// Load this run's captured inputs and settings into the draft.
    pub fn review_request(&self) {
        self.review_snapshot(false);
    }
    /// Load a settled Check run into the draft in Create mode.
    pub fn create_from_check(&self) {
        self.review_snapshot(true);
    }
    fn review_snapshot(&self, create: bool) {
        let snapshot = {
            let s = self.session.borrow();
            if s.is_closing() || s.state.confirmation.is_some() {
                return;
            }
            let Some(entry) = s
                .selected_entry()
                .filter(|entry| completion_state(entry).is_some() || entry.launch_error.is_some())
            else {
                return;
            };
            if create && !can_create(entry) {
                return;
            }
            let request = entry.request.request();
            RequestSnapshot::new(
                request.paths.clone(),
                match &request.output {
                    eml_core::OutputLocation::BesideSource => None,
                    eml_core::OutputLocation::Directory(path) => Some(path.clone()),
                },
                if create {
                    RunMode::Create
                } else {
                    request.mode
                },
            )
        };
        match snapshot {
            Ok(snapshot) if !self.session.borrow().state.selection.is_empty() => {
                self.set_confirmation(Some(state::PendingConfirmation::ReplaceDraft(snapshot)))
            }
            Ok(snapshot) => self.apply_snapshot(snapshot),
            Err(error) => self.message(&wording::queue_error(&error)),
        }
    }
    fn apply_snapshot(&self, snapshot: RequestSnapshot) {
        self.clear_operation();
        let mut s = self.session.borrow_mut();
        let request = snapshot.request();
        s.state.selection = request.paths.clone();
        s.state.mode = request.mode;
        s.state.view = state::View::Draft;
        let destination = match &request.output {
            eml_core::OutputLocation::BesideSource => None,
            eml_core::OutputLocation::Directory(path) => Some(path.clone()),
        };
        let result = s.preferences.set_destination(destination);
        drop(s);
        if let Err(error) = result {
            self.message(&error.to_string());
        }
        self.maintain_reports();
        self.refresh();
    }
    fn confirmation(&self) -> Confirmation {
        match &self.session.borrow().state.confirmation {
            None => Confirmation::None,
            Some(state::PendingConfirmation::Quit) => Confirmation::Quit,
            Some(state::PendingConfirmation::ForceStop(_)) => Confirmation::ForceStop,
            Some(state::PendingConfirmation::ReplaceDraft(_)) => Confirmation::ReplaceDraft,
        }
    }
    fn set_confirmation(&self, pending: Option<state::PendingConfirmation>) {
        if pending.is_some() {
            self.ui.set_drop_hovered(false);
        }
        self.session.borrow_mut().state.confirmation = pending;
        self.ui.set_confirmation(self.confirmation());
        self.refresh();
    }
    /// Whether native input and draft actions may proceed at this moment.
    pub fn inputs_enabled(&self) -> bool {
        let s = self.session.borrow();
        !s.is_closing() && s.state.confirmation.is_none()
    }
    /// Highlight the draft drop target while native hover events are available.
    pub fn hover_drop(&self, hovered: bool) {
        self.ui.set_drop_hovered(hovered && self.inputs_enabled());
    }
    /// The application-owned no-replace report naming window.
    pub fn export_window(&self) -> ExportWindow {
        self.export_ui.clone_strong()
    }
    /// Open a draft pinned to the exact current report, independently of navigation.
    pub fn begin_export(&self) {
        if self.session.borrow().is_closing() {
            return;
        }
        if self.export_draft.borrow().is_some() {
            let _ = self.export_ui.show();
            return;
        }
        let Some(source) = self.capture_export() else {
            return;
        };
        let directory = self.export_directory.borrow().clone();
        self.export_ui
            .set_run_label(format!("Run {}", source.run).into());
        self.export_ui
            .set_file_name(format!("eml-run-{}.jsonl", source.run).into());
        self.export_ui.set_directory(
            eml_protocol::TerminalPath::new(&directory)
                .to_string()
                .into(),
        );
        self.export_ui
            .set_notice("Choose a new file name. Existing entries remain unchanged.".into());
        self.export_ui.set_busy(false);
        *self.export_draft.borrow_mut() = Some(ExportDraft {
            token: Rc::new(()),
            source,
            directory,
        });
        if let Err(error) = self.export_ui.show() {
            self.cancel_export();
            self.message(&error.to_string());
        }
        self.refresh();
    }
    /// Capture folder-dialog ownership before an asynchronous native call.
    pub fn capture_export_folder(&self) -> Option<ExportFolderRequest> {
        if self.session.borrow().is_closing() || self.export_ui.get_busy() {
            return None;
        }
        self.export_draft
            .borrow()
            .as_ref()
            .map(|draft| ExportFolderRequest {
                token: Rc::downgrade(&draft.token),
            })
    }
    fn export_folder_is_current(&self, request: &ExportFolderRequest) -> bool {
        request.token.upgrade().is_some_and(|token| {
            self.export_draft
                .borrow()
                .as_ref()
                .is_some_and(|draft| Rc::ptr_eq(&token, &draft.token))
        })
    }
    /// Apply a native folder choice only to the originating, still-open draft.
    pub fn set_export_directory(&self, request: &ExportFolderRequest, directory: PathBuf) {
        if self.session.borrow().is_closing()
            || self.export_ui.get_busy()
            || !directory.is_absolute()
            || !self.export_folder_is_current(request)
        {
            return;
        }
        if let Some(draft) = self.export_draft.borrow_mut().as_mut() {
            draft.directory = directory.clone();
            *self.export_directory.borrow_mut() = directory.clone();
            self.export_ui.set_directory(
                eml_protocol::TerminalPath::new(&directory)
                    .to_string()
                    .into(),
            );
        }
    }
    /// Surface folder-dialog errors inside the export flow.
    pub fn export_notice(&self, request: &ExportFolderRequest, notice: &str) {
        if self.session.borrow().is_closing() || !self.export_folder_is_current(request) {
            return;
        }
        self.export_ui
            .set_notice(eml_protocol::DisplayText::new(notice).to_string().into());
    }
    /// Cancel a draft without cancelling an already-owned physical export.
    pub fn cancel_export(&self) {
        self.export_draft.borrow_mut().take();
        let _ = self.export_ui.hide();
        self.refresh();
    }
    fn create_export(&self) {
        if self.session.borrow().is_closing() || self.session.borrow().export.is_some() {
            return;
        }
        let name = self.export_ui.get_file_name();
        let path = Path::new(name.as_str());
        if name.contains('\0')
            || path.components().count() != 1
            || !matches!(
                path.components().next(),
                Some(std::path::Component::Normal(_))
            )
        {
            self.export_ui
                .set_notice("Enter one file name, without a folder or path separator.".into());
            return;
        }
        let Some((source, target)) = self
            .export_draft
            .borrow()
            .as_ref()
            .map(|draft| (draft.source.clone(), draft.directory.join(path)))
        else {
            return;
        };
        self.export_ui.set_busy(true);
        self.export_ui.set_notice("Creating the report…".into());
        self.start_captured_export(source, target);
        if self.session.borrow().export.is_none() {
            self.export_ui.set_busy(false);
        }
    }
    /// Native path for copying, without granting reveal qualification.
    pub fn selected_path(&self, destination: bool) -> Option<PathBuf> {
        let s = self.session.borrow();
        s.selected_entry()?
            .report
            .as_ref()?
            .with_report(|report| {
                let index = s.selected_entry()?.presentation.selected?;
                if destination {
                    report.destination_path(index)
                } else {
                    report.source_path(index)
                }
            })
            .flatten()
    }
    /// Capture the selected settled stream before presenting an export draft.
    pub fn capture_export(&self) -> Option<ReportExport> {
        let s = self.session.borrow();
        if s.export.is_some() || s.is_closing() {
            return None;
        }
        let entry = s.selected_entry()?;
        if entry.completion.as_ref()?.jsonl.is_empty() {
            return None;
        }
        let source = ReportExport {
            run: entry.id,
            jsonl: entry.completion.as_ref()?.jsonl.clone(),
        };
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
        let target = path.clone();
        match std::thread::Builder::new()
            .name("report-export".into())
            .spawn(move || eml_app::export_report(&path, &jsonl))
        {
            Ok(job) => {
                self.session.borrow_mut().export = Some(ExportJob {
                    run,
                    path: target,
                    task: job,
                });
                platform::set_busy(true);
            }
            Err(error) => {
                let message = format!("Could not start report export: {error}. Try again.");
                self.export_ui.set_notice(message.clone().into());
                self.message(&message);
            }
        }
        self.refresh();
    }
    /// Native destination from an earlier qualified copy receipt, for advisory reveal.
    pub fn selected_copy(&self) -> Option<PathBuf> {
        let s = self.session.borrow();
        let entry = s.selected_entry()?;
        entry
            .report
            .as_ref()?
            .with_report(|report| {
                entry
                    .presentation
                    .selected
                    .and_then(|index| report.reveal_path(index))
            })
            .flatten()
    }
    /// Last successfully saved report, kept as a native path for advisory reveal.
    pub fn saved_report_path(&self) -> Option<PathBuf> {
        let s = self.session.borrow();
        s.selected_entry()?.saved_report.clone()
    }
    // At most one OS launch request per initiating view. Launch acceptance is
    // advisory: it neither verifies the viewer nor changes processing receipts.
    #[cfg(any(feature = "desktop", test))]
    pub(crate) fn open_advisory(
        &self,
        target: OpenTarget,
        failure: &'static str,
        open: impl FnOnce() -> Result<(), String> + Send + 'static,
    ) {
        if self.session.borrow().is_closing()
            || self
                .open_jobs
                .borrow()
                .iter()
                .any(|job| job.target == target)
        {
            return;
        }
        self.set_open_feedback(target, "Sending open request…", "");
        let serial = self.operation_serial.get();
        match std::thread::Builder::new()
            .name("native-open".into())
            .spawn(open)
        {
            Ok(task) => {
                self.open_jobs.borrow_mut().push(OpenJob {
                    target,
                    serial,
                    failure,
                    task,
                });
                self.refresh();
            }
            Err(error) => self.set_open_feedback(target, failure, &error.to_string()),
        }
    }
    fn poll_open_jobs(&self) {
        let mut completed = Vec::new();
        {
            let mut jobs = self.open_jobs.borrow_mut();
            let mut index = 0;
            while index < jobs.len() {
                if jobs[index].task.is_finished() {
                    completed.push(jobs.remove(index));
                } else {
                    index += 1;
                }
            }
        }
        let changed = !completed.is_empty();
        for job in completed {
            if job.target == OpenTarget::Main && job.serial != self.operation_serial.get() {
                continue;
            }
            match job.task.join() {
                Ok(Ok(())) => self.set_open_feedback(job.target, "Open request sent.", ""),
                Ok(Err(error)) => self.set_open_feedback(job.target, job.failure, &error),
                Err(_) => self.set_open_feedback(
                    job.target,
                    job.failure,
                    "The native launch task did not complete.",
                ),
            }
        }
        if changed {
            self.refresh();
        }
    }
    fn set_open_feedback(&self, target: OpenTarget, message: &str, detail: &str) {
        let detail = eml_protocol::DisplayText::new(detail).to_string();
        match target {
            OpenTarget::Main => {
                self.message(message);
                self.ui.set_operation_details(detail.into());
            }
            #[cfg(feature = "desktop")]
            OpenTarget::About => {
                self.about.set_notice(message.into());
                self.about.set_notice_details(detail.into());
            }
            #[cfg(any(feature = "desktop", test))]
            OpenTarget::Licenses => {
                self.licenses.set_notice(message.into());
                self.licenses.set_notice_details(detail.into());
            }
        }
    }
    /// Show transient feedback for the current user operation, separate from run facts.
    pub fn message(&self, message: &str) {
        self.operation("Operation status", message);
    }
    fn operation(&self, label: &str, message: &str) {
        self.operation_serial
            .set(self.operation_serial.get().wrapping_add(1));
        self.ui.set_operation_details("".into());
        self.ui.set_operation_label(label.into());
        self.ui
            .set_operation_feedback(eml_protocol::DisplayText::new(message).to_string().into());
    }
    fn clear_operation(&self) {
        self.operation_serial
            .set(self.operation_serial.get().wrapping_add(1));
        self.ui.set_operation_label("".into());
        self.ui.set_operation_feedback("".into());
        self.ui.set_operation_details("".into());
    }
    fn dismiss(&self) {
        let index = self.session.borrow().selected_index();
        if let Some(index) = index {
            self.dismiss_run(index);
        }
    }
    fn dismiss_run(&self, index: usize) {
        self.clear_operation();
        let mut s = self.session.borrow_mut();
        let Some(entry) = s.entries.get(index) else {
            return;
        };
        let id = entry.id;
        if !self.can_close_run(&s, entry) {
            return;
        }
        if s.state.queue.dismiss(id) {
            s.entries.remove(index);
            if s.state.view == state::View::Run(id) {
                s.state.view = s
                    .entries
                    .get(index.min(s.entries.len().saturating_sub(1)))
                    .map_or(state::View::Draft, |entry| state::View::Run(entry.id));
            }
        }
        drop(s);
        self.maintain_reports();
        self.start_next();
        self.refresh();
    }
}
fn request_destination(request: &RequestSnapshot) -> String {
    match &request.request().output {
        eml_core::OutputLocation::BesideSource => "Next to originals".into(),
        eml_core::OutputLocation::Directory(path) => {
            eml_protocol::TerminalPath::new(path).to_string()
        }
    }
}
fn run_view(id: RunId, request: &RequestSnapshot, status: RunStatus) -> RunView {
    RunView {
        token: id.to_string().into(),
        waiting: status == RunStatus::Queued,
        closable: false,
        tone: match status {
            RunStatus::Queued => Tone::Neutral,
            RunStatus::Checking
            | RunStatus::Creating
            | RunStatus::StopRequested
            | RunStatus::ForceStopRequested => Tone::Running,
            RunStatus::Completed => Tone::Ok,
            RunStatus::Stopped | RunStatus::StoppedBeforeStart | RunStatus::ForceStopped => {
                Tone::Stopped
            }
            RunStatus::CompletedWithFailures
            | RunStatus::RequestRefused
            | RunStatus::Incomplete
            | RunStatus::IdentityFailure
            | RunStatus::StartFailed => Tone::Attention,
        },
        unseen: false,
        label: format!("Run {id}").into(),
        status: wording::phase(status).into(),
        mode: if request.request().mode == RunMode::Create {
            "Create copies"
        } else {
            "Check"
        }
        .into(),
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
    if let Some((state, failure)) = completion_state(entry) {
        if failure == Some(WorkerFailure::Identity) {
            return RunStatus::IdentityFailure;
        }
        return match state {
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
        if active.stop == state::Stop::Forced {
            return RunStatus::ForceStopRequested;
        }
        if matches!(active.stop, state::Stop::Requested { .. }) {
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
        && completion_state(entry).is_some_and(|(state, failure)| {
            failure.is_none()
                && matches!(
                    state,
                    WorkerState::Completed | WorkerState::CompletedWithFailures
                )
        })
}
fn completion_state(entry: &Entry) -> Option<(WorkerState, Option<WorkerFailure>)> {
    entry
        .completion
        .as_ref()
        .map(|c| (c.state, c.failure))
        .or_else(|| entry.stored.as_ref().map(|c| (c.state(), c.failure())))
}

#[cfg(test)]
mod tests;

fn now_ms() -> u64 {
    i_slint_core::animations::current_tick().as_millis()
}

fn reconcile_entry(entry: &mut Entry) {
    let Some(handle) = entry.report.clone() else {
        return;
    };
    handle.with_report(|report| {
        let (categories, tally, total) = view_model::report_facts(report);
        entry.presentation.tally = tally;
        entry.presentation.total = total;
        let default = entry.completion.is_some() && !entry.presentation.initialized;
        entry
            .presentation
            .reconcile(&categories, entry.completion.is_some());
        if default {
            entry.presentation.selected = present::table_rows(
                report,
                &entry.request,
                entry.presentation.filter,
                entry.presentation.sort,
            )
            .first()
            .map(|row| row.original_index as usize);
        }
    });
}
