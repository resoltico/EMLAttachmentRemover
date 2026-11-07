//! Exact settled-stream export with stale native-panel token rejection.
use super::*;
impl ReportExport {
    /// Default filename uses the user's local date.
    pub fn suggested_name(&self) -> String {
        format!("EML emails {}.jsonl", chrono::Local::now().date_naive())
    }
}
impl DesktopController {
    pub(super) fn settled_bytes(&self) -> Option<Arc<[u8]>> {
        let r = self.runtime.borrow();
        r.batch
            .create
            .as_ref()
            .and_then(|slot| slot.completion.as_ref())
            .or_else(|| {
                r.batch
                    .check
                    .as_ref()
                    .and_then(|slot| slot.completion.as_ref())
            })
            .map(|completion| completion.jsonl.clone())
    }
    /// Most recently selected report folder, separate from the copy location.
    pub fn report_directory(&self) -> PathBuf {
        self.runtime
            .borrow()
            .preferences
            .last_report_folder()
            .map_or_else(|| self.home.clone(), Path::to_owned)
    }
    /// Pin exact settled bytes during one native Save panel lifetime.
    pub fn capture_export(&self) -> Option<ReportExport> {
        if !self.can_save_report() {
            return None;
        }
        let source = ReportExport {
            jsonl: self.settled_bytes()?,
            token: Rc::new(()),
        };
        *self.pending_save.borrow_mut() = Some(source.clone());
        self.refresh();
        Some(source)
    }
    pub(super) fn export_is_current(&self, source: &ReportExport) -> bool {
        self.pending_save
            .borrow()
            .as_ref()
            .is_some_and(|p| Rc::ptr_eq(&p.token, &source.token))
    }
    /// Cancellation cannot invalidate a newer save panel.
    pub fn cancel_export(&self, source: &ReportExport) {
        if self.export_is_current(source) {
            self.pending_save.borrow_mut().take();
            self.refresh();
        }
    }
    /// Save to a path already approved by a native panel, including replacement.
    pub fn start_export(&self, path: PathBuf) {
        if let Some(source) = self.capture_export() {
            self.start_captured_export(source, path);
        }
    }
    /// Write only the current native panel's captured report.
    pub fn start_captured_export(&self, source: ReportExport, path: PathBuf) {
        if !self.export_is_current(&source) {
            return;
        }
        if self.locked() {
            self.cancel_export(&source);
            return;
        }
        self.pending_save.borrow_mut().take();
        if !path.is_absolute() {
            self.message(wording::ABSOLUTE_REPORT);
            return;
        }
        let mut r = self.runtime.borrow_mut();
        if r.export.is_some() {
            return;
        }
        if let Some(parent) = path.parent() {
            let _ = r.preferences.set_report_folder(parent.to_owned());
        }
        let target = path.clone();
        match std::thread::Builder::new()
            .name("report-export".into())
            .spawn(move || eml_app::export_report(&path, &source.jsonl))
        {
            Ok(task) => r.export = Some(ExportJob { path: target, task }),
            Err(error) => self.message(&wording::report_failed(&error.to_string())),
        }
        platform::set_busy(r.export.is_some() || r.active.is_some());
        drop(r);
        self.refresh();
    }
    pub(super) fn poll_export(&self) {
        let mut r = self.runtime.borrow_mut();
        if !r.export.as_ref().is_some_and(|job| job.task.is_finished()) {
            return;
        }
        let job = r.export.take().unwrap();
        drop(r);
        match job.task.join() {
            Ok(Ok(())) => {
                *self.saved_report.borrow_mut() = Some(job.path);
                self.message(wording::REPORT_SAVED);
            }
            Ok(Err(error)) => self.message(&wording::report_failed(&error.to_string())),
            Err(_) => self.message(&wording::report_failed(wording::EXPORT_FAILED)),
        }
    }
    /// Native path of the most recent successful report save.
    pub fn saved_report_path(&self) -> Option<PathBuf> {
        self.saved_report.borrow().clone()
    }
}
