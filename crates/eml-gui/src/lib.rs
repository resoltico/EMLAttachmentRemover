//! One-batch desktop renderer of in-process core events.
#![deny(unsafe_code)]
mod actions;
#[cfg(feature = "native")]
mod desktop;
#[cfg(feature = "native")]
pub use desktop::run_desktop;
mod callbacks;
pub mod ingress;
#[cfg(feature = "native")]
mod native_dialogs;
pub mod platform;
pub mod preferences;
mod present;
mod report_export;
mod state;
mod text_commands;
mod wording;
mod worker;
use eml_core::RunMode;
use eml_core::{OutputLocation, RequestPathError, RunRequest};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use state::{Batch, Effect, Kind, State};
use std::{
    cell::{Cell, RefCell},
    io,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
    thread::JoinHandle,
};
pub use worker::RunSink;
use worker::{Runner, WorkerRun, WorkerState};
#[expect(
    missing_docs,
    unreachable_pub,
    reason = "Slint generates public UI bindings"
)]
mod ui {
    slint::include_modules!();
}
pub use ui::*;
pub use wording::HOME_UNAVAILABLE;
struct Active {
    kind: Kind,
    generation: u64,
    worker: WorkerRun,
    stop_at: Option<u64>,
    estimate: state::ProgressEstimate,
}
struct ExportJob {
    path: PathBuf,
    task: JoinHandle<io::Result<()>>,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum OpenTarget {
    Main,
    #[cfg(any(feature = "native", test))]
    About,
    #[cfg(any(feature = "native", test))]
    Licenses,
}
struct OpenJob {
    target: OpenTarget,
    generation: u64,
    failure: &'static str,
    task: JoinHandle<Result<(), String>>,
}
struct Runtime {
    batch: Batch,
    active: Option<Active>,
    retired_check: Option<WorkerRun>,
    force_exit: bool,
    preferences: preferences::Preferences,
    export: Option<ExportJob>,
    quit: bool,
    quitting: bool,
    quit_at: Option<u64>,
    should_exit: bool,
}
/// Immutable bytes and unique ownership token for a native Save Report panel.
#[derive(Clone)]
pub struct ReportExport {
    epoch: u64,
    jsonl: Arc<[u8]>,
    token: Rc<()>,
}
/// The single owner of batch transitions, core runs and UI actions.
pub struct DesktopController {
    ui: MainWindow,
    about: AboutWindow,
    licenses: LicenseWindow,
    runtime: RefCell<Runtime>,
    runner: Arc<Runner>,
    home: PathBuf,
    rows: RefCell<Vec<present::Row>>,
    model: Rc<VecModel<EmailRow>>,
    pending_save: RefCell<Option<ReportExport>>,
    drops: RefCell<ingress::DropBatch>,
    notice: RefCell<Option<(String, u64)>>,
    saved_report: RefCell<Option<PathBuf>>,
    open_jobs: RefCell<Vec<OpenJob>>,
    open_feedback_generation: Cell<u64>,
    panel_epoch: Cell<u64>,
    presentation_dirty: Cell<bool>,
    rendered_revision: Cell<u64>,
}
fn now_ms() -> u64 {
    i_slint_core::animations::current_tick().as_millis()
}
impl DesktopController {
    /// Construct the production view and controller without starting work.
    pub fn new(
        runner: impl Fn(
            &RunRequest,
            &eml_core::RunControl,
            &mut RunSink,
        ) -> Result<eml_core::RunOutcome, eml_core::InvalidRequest>
        + Send
        + Sync
        + 'static,
        home: PathBuf,
        preferences: preferences::Preferences,
    ) -> Result<Rc<Self>, slint::PlatformError> {
        let ui = MainWindow::new()?;
        let about = AboutWindow::new()?;
        let licenses = LicenseWindow::new()?;
        ui.set_product_name(eml_core::product::DISPLAY_NAME.into());
        ui.set_native_about(cfg!(all(feature = "native", target_os = "macos")));
        ui.set_words(wording::controls());
        about.set_words(wording::controls());
        licenses.set_words(wording::controls());
        licenses.set_notice(wording::controls().license_notice);
        about.set_product_name(eml_core::product::DISPLAY_NAME.into());
        about.set_version(eml_core::product::ABOUT_VERSION.as_str().into());
        about.set_copyright(
            include_str!("../../../LICENSE")
                .lines()
                .nth(1)
                .unwrap_or_default()
                .replace("(c)", "©")
                .into(),
        );
        licenses.set_licenses(include_str!("../../../LICENSE").into());
        let destination = preferences.destination().map(Path::to_owned);
        let mut batch = Batch::default();
        batch.destination = destination;
        let c = Rc::new(Self {
            ui,
            about,
            licenses,
            runner: Arc::new(runner),
            home,
            model: Rc::new(VecModel::default()),
            runtime: RefCell::new(Runtime {
                batch,
                active: None,
                retired_check: None,
                force_exit: false,
                preferences,
                export: None,
                quit: false,
                quitting: false,
                quit_at: None,
                should_exit: false,
            }),
            rows: RefCell::default(),
            pending_save: RefCell::default(),
            drops: RefCell::default(),
            notice: RefCell::default(),
            saved_report: RefCell::default(),
            open_jobs: RefCell::default(),
            open_feedback_generation: Cell::new(0),
            panel_epoch: Cell::new(0),
            presentation_dirty: Cell::new(true),
            rendered_revision: Cell::new(0),
        });
        c.ui.set_rows(c.model.clone().into());
        c.bind();
        c.refresh();
        Ok(c)
    }
    /// Main desktop window.
    pub fn window(&self) -> MainWindow {
        self.ui.clone_strong()
    }
    /// Application About window.
    pub fn about_window(&self) -> AboutWindow {
        self.about.clone_strong()
    }
    /// Bundled license viewer.
    pub fn license_window(&self) -> LicenseWindow {
        self.licenses.clone_strong()
    }
    /// Admit native paths through the same boundary used by dialogs and documents.
    pub fn stage(&self, paths: Vec<PathBuf>) -> Result<(), RequestPathError> {
        if paths.is_empty() {
            self.message(wording::NOTHING_ADDED);
            self.refresh();
            return Ok(());
        }
        if !self.inputs_enabled() {
            self.message(wording::BUSY_ADDING);
            return Ok(());
        }
        let mut runtime = self.runtime.borrow_mut();
        let mut combined = if matches!(runtime.batch.state(), State::Done | State::Stopped) {
            Vec::new()
        } else {
            runtime.batch.inputs.clone()
        };
        let mut seen: std::collections::HashSet<_> = combined.iter().cloned().collect();
        for path in paths {
            if seen.insert(path.clone()) {
                combined.push(path);
                if combined.len() > eml_core::MAX_INPUT_PATHS {
                    return Err(RequestPathError::TooManyInputs);
                }
            }
        }
        RunRequest::from_absolute(
            combined.clone(),
            runtime
                .batch
                .destination
                .clone()
                .map_or(OutputLocation::BesideSource, OutputLocation::Directory),
            RunMode::DryRun,
        )?;
        let effect = runtime.batch.add(combined);
        drop(runtime);
        self.changed();
        self.effect(effect);
        self.start_pending();
        self.refresh();
        Ok(())
    }
    fn changed(&self) {
        self.invalidate_open_feedback();
        self.notice.borrow_mut().take();
        self.saved_report.borrow_mut().take();
    }
    fn effect(&self, effect: Option<Effect>) {
        match effect {
            Some(Effect::InterruptCheck) => {
                let mut r = self.runtime.borrow_mut();
                if let Some(mut old) = r.active.take() {
                    debug_assert!(old.kind == Kind::Check && r.retired_check.is_none());
                    old.worker.stop();
                    r.retired_check = Some(old.worker);
                    r.batch.active = None;
                }
            }
            Some(Effect::BusyAdding) => self.message(wording::BUSY_ADDING),
            None => {}
        }
    }
    /// Change the save location and schedule a fresh read-only preview.
    pub fn set_destination(&self, destination: Option<PathBuf>) {
        if !self.can_destination() {
            return;
        }
        if let Some(path) = &destination
            && let Err(error) = eml_core::validate_save_folder_path(path)
        {
            self.message(&error.to_string());
            return;
        }
        let mut runtime = self.runtime.borrow_mut();
        let result = runtime.preferences.set_destination(destination.clone());
        runtime.batch.destination = destination;
        let effect = runtime.batch.changed();
        drop(runtime);
        self.changed();
        self.effect(effect);
        if let Err(e) = result {
            self.message(&eml_core::vocabulary::interface_io_error(
                eml_core::vocabulary::InterfaceIo::SaveSettings,
                &e,
            ));
        }
        self.start_pending();
        self.refresh();
    }
    /// Clear this batch, cancelling a read-only preview if needed.
    pub fn start_over(&self) {
        if !self.can_reset() {
            return;
        }
        let effect = self.runtime.borrow_mut().batch.reset();
        self.changed();
        self.effect(effect);
        self.refresh();
    }
    /// Remove attachments, or continue an interrupted create with the same inputs.
    pub fn start_selection(&self) {
        if !self.can_start() {
            return;
        }
        let effect = self.runtime.borrow_mut().batch.remove();
        self.effect(effect);
        self.start_pending();
        self.refresh();
    }
    fn start_pending(&self) {
        let mut runtime = self.runtime.borrow_mut();
        if runtime.active.is_some()
            || runtime.quitting
            || (runtime.batch.pending == Some(Kind::Check) && runtime.retired_check.is_some())
        {
            return;
        }
        let Some((kind, generation)) = runtime.batch.start() else {
            return;
        };
        let request = RunRequest::from_absolute(
            runtime.batch.inputs.clone(),
            runtime
                .batch
                .destination
                .clone()
                .map_or(OutputLocation::BesideSource, OutputLocation::Directory),
            if kind == Kind::Check {
                RunMode::DryRun
            } else {
                RunMode::Create
            },
        );
        let result = request
            .map_err(|e| eml_core::InvalidRequest::Paths(e).into_failure())
            .and_then(|request| {
                WorkerRun::start(self.runner.clone(), request)
                    .map_err(eml_core::OperationFailure::delivery)
            });
        match result {
            Ok(worker) => {
                runtime.batch.slot_mut(kind).unwrap().report = Some(worker.report().clone());
                runtime.active = Some(Active {
                    kind,
                    generation,
                    worker,
                    stop_at: None,
                    estimate: state::ProgressEstimate::start(now_ms()),
                });
            }
            Err(error) => {
                if kind == Kind::Create {
                    runtime.batch.previous_create = None;
                }
                let completion = worker::WorkerCompletion::rejected(error);
                let slot = runtime.batch.slot_mut(kind).unwrap();
                slot.launch_error = true;
                slot.report = Some(completion.report.clone());
                slot.completion = Some(completion);
                runtime.batch.active = None;
            }
        }
        platform::set_busy(runtime.active.is_some() || runtime.export.is_some());
    }
    fn interrupt(&self) {
        let mut runtime = self.runtime.borrow_mut();
        if let Some(active) = &mut runtime.active
            && active.stop_at.is_none()
        {
            active.stop_at = Some(now_ms());
            active.worker.stop();
        }
    }
    /// Cooperative create cancellation, sent once.
    pub fn stop(&self) {
        if self.can_stop() {
            self.interrupt();
            self.refresh();
        }
    }
    /// Drain typed observations and settle workers before dispatching pending work.
    pub fn tick(&self) {
        let expired = self
            .notice
            .borrow()
            .as_ref()
            .is_some_and(|(_, at)| now_ms().saturating_sub(*at) >= 6000);
        if expired {
            self.notice.borrow_mut().take();
            self.presentation_dirty.set(true);
        }
        platform::refresh_menu();
        if platform::take_about() {
            self.show_about();
        }
        if platform::take_quit() {
            self.request_quit();
        }
        let opened = platform::take_opened();
        if !opened.is_empty() {
            self.open_documents(opened);
        }
        self.poll_export();
        self.poll_open_jobs();
        {
            let mut runtime = self.runtime.borrow_mut();
            let active = runtime.active.as_mut();
            if let Some(active) = active {
                let processed = active.worker.report().with_report(|r| r.items().len());
                active.estimate.observe(now_ms(), processed as u32);
                if let Some(completion) = active.worker.poll() {
                    let stopped = completion.state == WorkerState::Stopped;
                    let (kind, generation) = (active.kind, active.generation);
                    runtime.batch.settle(kind, generation, completion, stopped);
                    runtime.active = None;
                }
            }
            if let Some(old) = &mut runtime.retired_check
                && old.poll().is_some()
            {
                runtime.retired_check = None;
            }
            if runtime.quitting
                && (runtime.active.is_none() || runtime.force_exit)
                && runtime.export.is_none()
            {
                runtime.should_exit = true;
            }
            platform::set_busy(runtime.active.is_some() || runtime.export.is_some());
        }
        self.start_pending();
        if self.is_busy()
            || self.runtime.borrow().batch.revision() != self.rendered_revision.get()
            || self.presentation_dirty.get()
        {
            self.refresh();
        }
    }
    /// Whether an owned worker, pending worker or report writer remains active.
    pub fn is_busy(&self) -> bool {
        let r = self.runtime.borrow();
        r.active.is_some() || r.batch.pending.is_some() || r.export.is_some()
    }
    /// Apply create confirmation rules; obsolete checks receive cooperative stop.
    pub fn request_quit(&self) -> bool {
        let mut r = self.runtime.borrow_mut();
        if r.quitting {
            return false;
        }
        self.panel_epoch.set(self.panel_epoch.get().wrapping_add(1));
        self.drops.borrow_mut().cancel();
        if r.active.as_ref().is_some_and(|a| a.kind == Kind::Create) || r.export.is_some() {
            r.quit = true;
            drop(r);
            let _ = self.about.hide();
            let _ = self.licenses.hide();
            let _ = self.ui.show();
            self.ui.window().set_minimized(false);
            self.refresh();
            false
        } else {
            r.quitting = true;
            r.batch.pending = None;
            if let Some(mut active) = r.active.take() {
                active.worker.stop();
                debug_assert!(active.kind == Kind::Check && r.retired_check.is_none());
                r.retired_check = Some(active.worker);
                r.batch.active = None;
            }
            r.should_exit = r.export.is_none();
            let ready = r.should_exit;
            drop(r);
            self.refresh();
            ready
        }
    }
    /// Cancel the Quit sheet; late native panels remain invalid.
    pub fn cancel_quit(&self) {
        let mut r = self.runtime.borrow_mut();
        if !r.quitting {
            r.quit = false;
        }
        drop(r);
        self.refresh();
    }
    /// Stop cooperatively and wait for the create and owned exports before quitting.
    pub fn stop_and_quit(&self) {
        {
            let mut r = self.runtime.borrow_mut();
            if !r.quit {
                return;
            }
            r.quitting = true;
            r.quit_at = Some(now_ms());
            r.batch.pending = None;
        }
        self.interrupt();
        self.refresh();
    }
    /// Exit after five seconds without awaiting the create; owned exports still settle.
    pub fn quit_now(&self) {
        if !self.can_quit_now() {
            return;
        }
        let mut r = self.runtime.borrow_mut();
        r.quitting = true;
        r.force_exit = true;
        r.should_exit = r.export.is_none();
        drop(r);
        self.refresh();
    }
    /// Whether the event loop may exit, including an explicitly confirmed Quit Now.
    pub fn should_exit(&self) -> bool {
        self.runtime.borrow().should_exit
    }
    /// Transient user feedback, outside report facts.
    pub fn message(&self, text: &str) {
        self.invalidate_open_feedback();
        self.presentation_dirty.set(true);
        *self.notice.borrow_mut() = Some((
            eml_core::display::DisplayText::new(text).to_string(),
            now_ms(),
        ));
    }
    /// Informational feedback from native adapters.
    pub fn success(&self, text: &str) {
        self.message(text);
    }
    /// Native document events share the drop and panel admission rules.
    pub fn open_documents(&self, paths: Vec<PathBuf>) {
        if let Err(error) = self.stage(paths) {
            self.message(&wording::request_error(&error));
        }
    }
    /// Native drop hover shares admission with all other sources.
    pub fn hover_drop(&self, hovered: bool) {
        self.ui.set_drop_hovered(hovered && self.inputs_enabled());
    }
    /// Batch all paths in a single native window drop.
    pub fn deliver_drop(self: &Rc<Self>, path: PathBuf) {
        if !self.inputs_enabled() {
            self.message(wording::BUSY_ADDING);
            return;
        }
        if self.drops.borrow_mut().push(path) {
            let weak = Rc::downgrade(self);
            slint::Timer::single_shot(std::time::Duration::ZERO, move || {
                if let Some(c) = weak.upgrade() {
                    let paths = c.drops.borrow_mut().commit();
                    if let Some(paths) = paths
                        && let Err(e) = c.stage(paths)
                    {
                        c.message(&wording::request_error(&e));
                    }
                    c.hover_drop(false);
                }
            });
        }
    }
    /// Cancel a native hover without changing the current batch.
    pub fn cancel_drop(&self) {
        self.drops.borrow_mut().cancel();
        self.ui.set_drop_hovered(false);
    }
    /// Epoch used to reject native-panel results delivered after a Quit decision.
    pub fn panel_epoch(&self) -> u64 {
        self.panel_epoch.get()
    }
    /// Selected native source, never reconstructed from display text.
    pub fn selected_original(&self) -> Option<PathBuf> {
        self.runtime.borrow().batch.selected.clone()
    }
    /// Selected copy qualified by a receipt from the current create.
    pub fn selected_copy(&self) -> Option<PathBuf> {
        let selected = self.selected_original()?;
        self.rows
            .borrow()
            .iter()
            .find(|row| row.key == selected)?
            .copy
            .clone()
    }
    /// All distinct verified output roots for the current batch.
    pub fn output_roots(&self) -> Vec<PathBuf> {
        let mut roots = Vec::new();
        for row in self.rows.borrow().iter() {
            if let Some(parent) = row.copy.as_deref().and_then(Path::parent)
                && !roots.iter().any(|p| p == parent)
            {
                roots.push(parent.to_owned());
            }
        }
        roots
    }
    /// Full native diagnostics for the selected row.
    pub fn clipboard_text(&self) -> Option<String> {
        let selected = self.selected_original()?;
        self.rows
            .borrow()
            .iter()
            .find(|r| r.key == selected)
            .map(present::Row::details)
    }
    fn selected(&self, key: &str) {
        let mut r = self.runtime.borrow_mut();
        let path = self
            .rows
            .borrow()
            .iter()
            .find(|row| row_key(&row.key) == key)
            .map(|row| row.key.clone());
        if r.batch.selected == path {
            r.batch.select(None);
        } else {
            r.batch.select(path);
        }
        drop(r);
        self.refresh();
    }
    fn navigate(&self, delta: i32, edge: i32) {
        let rows = self.rows.borrow();
        let mut r = self.runtime.borrow_mut();
        let visible: Vec<_> = rows
            .iter()
            .filter(|row| !r.batch.problems_only || row.problem())
            .collect();
        if visible.is_empty() {
            return;
        }
        let current = visible
            .iter()
            .position(|row| Some(&row.key) == r.batch.selected.as_ref())
            .map(|i| i as i32);
        let index = if edge < 0 {
            0
        } else if edge > 0 {
            visible.len() as i32 - 1
        } else {
            current.unwrap_or(if delta < 0 { visible.len() as i32 } else { -1 }) + delta
        }
        .clamp(0, visible.len() as i32 - 1);
        r.batch.select(Some(visible[index as usize].key.clone()));
        drop(r);
        drop(rows);
        self.refresh();
    }
    fn invalidate_open_feedback(&self) {
        self.open_feedback_generation
            .set(self.open_feedback_generation.get().wrapping_add(1));
    }
    fn set_open_feedback(&self, target: OpenTarget, text: &str, detail: &str) {
        let detail = eml_core::display::DisplayText::new(detail).to_string();
        match target {
            OpenTarget::Main => self.message(&eml_core::vocabulary::open_feedback(text, &detail)),
            #[cfg(any(feature = "native", test))]
            OpenTarget::About => {
                self.about.set_notice(text.into());
                self.about.set_notice_details(detail.into());
            }
            #[cfg(any(feature = "native", test))]
            OpenTarget::Licenses => {
                self.licenses.set_notice(text.into());
                self.licenses.set_notice_details(detail.into());
            }
        }
    }
    #[cfg(any(feature = "native", test))]
    pub(crate) fn open_advisory(
        &self,
        target: OpenTarget,
        failure: &'static str,
        open: impl FnOnce() -> Result<(), String> + Send + 'static,
    ) {
        if self.runtime.borrow().quitting
            || self
                .open_jobs
                .borrow()
                .iter()
                .any(|job| job.target == target)
        {
            return;
        }
        self.set_open_feedback(target, wording::OPENING, "");
        match std::thread::Builder::new()
            .name("native-open".into())
            .spawn(open)
        {
            Ok(task) => self.open_jobs.borrow_mut().push(OpenJob {
                target,
                generation: self.open_feedback_generation.get(),
                failure,
                task,
            }),
            Err(error) => self.set_open_feedback(target, failure, &error.to_string()),
        }
    }
    fn poll_open_jobs(&self) {
        let mut jobs = self.open_jobs.borrow_mut();
        let mut index = 0;
        while index < jobs.len() {
            if !jobs[index].task.is_finished() {
                index += 1;
                continue;
            }
            let job = jobs.remove(index);
            if job.target == OpenTarget::Main
                && job.generation != self.open_feedback_generation.get()
            {
                continue;
            }
            match job.task.join() {
                Ok(Ok(())) => {
                    self.set_open_feedback(job.target, eml_core::vocabulary::OPEN_SENT, "")
                }
                Ok(Err(error)) => self.set_open_feedback(job.target, job.failure, &error),
                Err(_) => self.set_open_feedback(
                    job.target,
                    job.failure,
                    eml_core::vocabulary::OPEN_FAILED,
                ),
            }
        }
    }
}
fn row_key(path: &Path) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(path.as_os_str().as_encoded_bytes())
}
fn strings(values: impl IntoIterator<Item = String>) -> ModelRc<slint::SharedString> {
    Rc::new(VecModel::from(
        values.into_iter().map(Into::into).collect::<Vec<_>>(),
    ))
    .into()
}

#[cfg(test)]
mod tests;
