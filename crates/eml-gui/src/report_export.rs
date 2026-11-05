//! Native-save ownership and atomic exact-stream export, independent of later navigation.
use super::*;
impl ReportExport {
    /// Default native-panel filename using the user's local calendar date.
    pub fn suggested_name(&self) -> String {
        self.name_on(chrono::Local::now().date_naive())
    }
    fn name_on(&self, date: chrono::NaiveDate) -> String {
        format!("EML Run {} {date}.jsonl", self.run)
    }
}
impl DesktopController {
    /// Folder remembered for native report saving, independent of copy destinations.
    pub fn report_directory(&self) -> PathBuf {
        self.session
            .borrow()
            .preferences
            .last_report_folder()
            .map_or_else(|| self.home.clone(), Path::to_owned)
    }
    /// Capture one immutable stream and pin its result while the native panel is open.
    pub fn capture_export(&self) -> Option<ReportExport> {
        if !self.can_save_report() {
            return None;
        }
        let source = {
            let s = self.session.borrow();
            let entry = s.selected_entry()?;
            ReportExport {
                run: entry.id,
                jsonl: entry.completion.as_ref()?.jsonl.clone(),
                token: Rc::new(()),
            }
        };
        *self.pending_save.borrow_mut() = Some(source.clone());
        self.refresh();
        Some(source)
    }
    fn export_is_current(&self, source: &ReportExport) -> bool {
        self.pending_save
            .borrow()
            .as_ref()
            .is_some_and(|pending| Rc::ptr_eq(&pending.token, &source.token))
    }
    /// Ignore cancellation from an earlier save lifetime without touching a newer panel.
    pub fn cancel_export(&self, source: &ReportExport) {
        if self.export_is_current(source) {
            self.pending_save.borrow_mut().take();
            self.refresh();
        }
    }
    /// Save a selected report to an already-approved native-panel path.
    pub fn start_export(&self, path: PathBuf) {
        if let Some(source) = self.capture_export() {
            self.start_captured_export(source, path);
        }
    }
    /// Write the captured stream only for its live panel. Native Replace grants path replacement.
    pub fn start_captured_export(&self, source: ReportExport, path: PathBuf) {
        if self.session.borrow().is_closing() {
            self.cancel_export(&source);
            return;
        }
        if !self.export_is_current(&source) {
            return;
        }
        self.pending_save.borrow_mut().take();
        self.clear_operation();
        if !path.is_absolute() {
            self.failure("Save Report", "Choose an absolute report path.", "");
            self.refresh();
            return;
        }
        if self.session.borrow().export.is_some() {
            self.failure("Save Report", "A report is already being saved.", "");
            self.refresh();
            return;
        }
        if let Some(parent) = path.parent() {
            // Unavailable preferences stay preserved; the choice still applies to this session.
            let _ = self
                .session
                .borrow_mut()
                .preferences
                .set_report_folder(parent.to_owned());
        }
        let run = source.run;
        let jsonl = source.jsonl;
        let target = path.clone();
        match std::thread::Builder::new()
            .name("report-export".into())
            .spawn(move || eml_app::export_report(&path, &jsonl))
        {
            Ok(task) => {
                self.session.borrow_mut().export = Some(ExportJob {
                    run,
                    path: target,
                    task,
                });
                platform::set_busy(true);
            }
            Err(error) => self.failure(
                "Save Report",
                "Couldn’t start saving the report. Try again.",
                &error.to_string(),
            ),
        }
        self.refresh();
    }
    pub(super) fn poll_export(&self) -> bool {
        let job = {
            let mut s = self.session.borrow_mut();
            if !s.export.as_ref().is_some_and(|job| job.task.is_finished()) {
                return false;
            }
            s.export.take().expect("finished export")
        };
        match job.task.join() {
            Ok(Ok(()))=>self.set_feedback(state::Feedback::SavedReport{run:job.run,path:job.path,at:now_ms()}),
            Ok(Err(error))=>self.failure(&format!("Report for Run {}",job.run),"Couldn’t save the report. Check free space and permissions, or choose another folder.",&error.to_string()),
            Err(_)=>self.failure(&format!("Report for Run {}",job.run),"The report could not be saved. Try again.","The report writer did not complete."),
        }
        true
    }
    /// Exact native path belonging to the current successful save feedback.
    pub fn saved_report_path(&self) -> Option<PathBuf> {
        match &*self.feedback.borrow() {
            state::Feedback::SavedReport { path, .. } => Some(path.clone()),
            state::Feedback::None | state::Feedback::Message { .. } => None,
        }
    }
}
