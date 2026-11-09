//! Exact settled-stream export with stale native-panel token rejection.
use super::*;
impl ReportExport {
    /// Default filename uses the user's local date.
    pub fn suggested_name(&self) -> String {
        crate::wording::report_filename(chrono::Local::now().date_naive())
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
            epoch: self.panel_epoch(),
            jsonl: self.settled_bytes()?,
            token: Rc::new(()),
        };
        *self.pending_save.borrow_mut() = Some(source.clone());
        self.refresh();
        Some(source)
    }
    pub(super) fn owns_export_panel(&self, source: &ReportExport) -> bool {
        self.pending_save
            .borrow()
            .as_ref()
            .is_some_and(|p| Rc::ptr_eq(&p.token, &source.token))
    }
    /// Cancellation cannot invalidate a newer save panel.
    pub fn cancel_export(&self, source: &ReportExport) {
        if self.owns_export_panel(source) {
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
        if !self.owns_export_panel(&source) {
            return;
        }
        if self.locked() || source.epoch != self.panel_epoch() {
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
            .spawn(move || export_report(&path, &source.jsonl))
        {
            Ok(task) => r.export = Some(ExportJob { path: target, task }),
            Err(error) => self.message(&eml_core::vocabulary::interface_io_error(
                eml_core::vocabulary::InterfaceIo::SaveReport,
                &error,
            )),
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
            Ok(Err(error)) => self.message(&eml_core::vocabulary::interface_io_error(
                eml_core::vocabulary::InterfaceIo::SaveReport,
                &error,
            )),
            Err(_) => self.message(eml_core::vocabulary::REPORT_WRITER_STOPPED),
        }
    }
    /// Native path of the most recent successful report save.
    pub fn saved_report_path(&self) -> Option<PathBuf> {
        self.saved_report.borrow().clone()
    }
}

use std::io::Write;
/// Save the exact received JSONL, including an incomplete stream when a run failed.
/// The caller owns approval of the target, including replacement.
/// The final name exposes only complete, file-synchronized bytes. This does not
/// promise that the directory entry survives power loss. A crash can leave an
/// owned private staging file beside the requested destination.
fn export_report(path: &Path, jsonl: &[u8]) -> io::Result<()> {
    let directory = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut stage = tempfile::Builder::new()
        .prefix(".emlattachmentremover-report-")
        .suffix(".tmp")
        .tempfile_in(directory)?;
    if let Err(error) = stage
        .as_file_mut()
        .write_all(jsonl)
        .and_then(|()| stage.as_file().sync_all())
    {
        return Err(discard(stage, error));
    }
    stage
        .persist(path)
        .map(|_| ())
        .map_err(|failure| discard(failure.file, failure.error))
}

fn discard(stage: tempfile::NamedTempFile, error: io::Error) -> io::Error {
    let path = stage.path().to_path_buf();
    match stage.close() {
        Ok(()) => error,
        Err(cleanup) => eml_core::report_cleanup_failure(error, path, cleanup),
    }
}
