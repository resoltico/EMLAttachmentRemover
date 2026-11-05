//! Private session report storage and serial worker launch.
use super::*;
impl DesktopController {
    pub(super) fn start_next(&self) {
        let mut s = self.session.borrow_mut();
        if s.is_closing()
            || matches!(s.state.confirmation, Some(state::PendingConfirmation::Quit))
            || s.report_task.is_some()
            || s.memory_full()
        {
            return;
        }
        let next = match s.state.queue.start_next() {
            Ok(value) => value,
            Err(QueueError::StoredBytesFull) => return,
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
            presentation: state::RunPresentation::default(),
            storage_error: None,
            load_error: None,
            stored: None,
            saved_report: None,
        };
        match WorkerRun::start(&self.cli, &entry.request, &self.home) {
            Ok(worker) => {
                entry.report = Some(worker.report().clone());
                s.active = Some(Active {
                    id,
                    worker,
                    stop: state::Stop::Running,
                    estimate: state::ProgressEstimate::start(now_ms()),
                });
            }
            Err(error) => {
                entry.launch_error = Some(error.to_string());
                let _ = s.state.queue.complete(id);
            }
        }
        s.state.has_started = true;
        s.entries.push(entry);
        if s.state.view == state::View::Waiting(id) {
            s.state.view = state::View::Run(id);
        }
        self.clear_operation();
        platform::set_busy(s.needs_quit_confirmation());
    }
    pub(super) fn maintain_reports(&self) {
        let mut s = self.session.borrow_mut();
        if s.is_closing() {
            return;
        }
        if s.report_task.as_ref().is_some_and(ReportTask::is_finished) {
            let task = s.report_task.take().expect("finished report task");
            match task {
                ReportTask::Store(id, task) => {
                    let result = task.join().unwrap_or_else(|_| {
                        Err(io::Error::other(
                            "The session report writer did not complete.",
                        ))
                    });
                    if let Some(entry) = s.entries.iter_mut().find(|entry| entry.id == id) {
                        match result {
                            Ok(stored) => {
                                let bytes = stored.byte_len();
                                entry.stored = Some(stored);
                                s.state
                                    .queue
                                    .store_report(id, bytes)
                                    .expect("storage budget checked before write");
                            }
                            Err(error) => entry.storage_error = Some(error.to_string()),
                        }
                    }
                }
                ReportTask::Load(id, task) => {
                    let result = task.join().unwrap_or_else(|_| {
                        Err(io::Error::other(
                            "The session report reader did not complete.",
                        ))
                    });
                    if let Some(entry) = s.selected_entry_mut().filter(|entry| entry.id == id) {
                        match result {
                            Ok(completion) => {
                                entry.report = Some(completion.report.clone());
                                entry.completion = Some(completion);
                                reconcile_entry(entry);
                            }
                            Err(error) => {
                                s.entries
                                    .iter_mut()
                                    .find(|entry| entry.id == id)
                                    .expect("retained report")
                                    .load_error = Some(error.to_string())
                            }
                        }
                    }
                }
            }
        }
        let selected = s.selected_index();
        for (index, entry) in s.entries.iter_mut().enumerate() {
            if Some(index) != selected && entry.stored.is_some() {
                entry.completion = None;
                entry.report = None;
            }
        }
        if s.report_task.is_some() {
            return;
        }
        if let Some(entry) = s.entries.iter().find(|entry| {
            entry.completion.is_some() && entry.stored.is_none() && entry.storage_error.is_none()
        }) {
            let id = entry.id;
            let completion = entry.completion.as_ref().expect("settled report").clone();
            if completion.jsonl.len() as u64
                > eml_app::MAX_STORED_REPORT_BYTES - s.state.queue.stored_bytes()
            {
                s.entries
                    .iter_mut()
                    .find(|entry| entry.id == id)
                    .expect("retained report")
                    .storage_error = Some(
                    "Session report storage is full. Close a finished run to add more.".into(),
                );
                return;
            }
            let mode = entry.request.request().mode;
            match std::thread::Builder::new()
                .name("session-report-write".into())
                .spawn(move || eml_app::StoredReport::write(&completion, mode))
            {
                Ok(task) => s.report_task = Some(ReportTask::Store(id, task)),
                Err(error) => {
                    s.entries
                        .iter_mut()
                        .find(|entry| entry.id == id)
                        .expect("retained report")
                        .storage_error = Some(error.to_string())
                }
            }
        } else if !s.memory_full()
            && let Some(entry) = s
                .selected_entry()
                .filter(|entry| entry.report.is_none() && entry.load_error.is_none())
            && let Some(stored) = entry.stored.clone()
        {
            let id = entry.id;
            match std::thread::Builder::new()
                .name("session-report-read".into())
                .spawn(move || stored.load())
            {
                Ok(task) => s.report_task = Some(ReportTask::Load(id, task)),
                Err(error) => {
                    s.entries
                        .iter_mut()
                        .find(|entry| entry.id == id)
                        .expect("retained report")
                        .load_error = Some(error.to_string())
                }
            }
        }
    }
}
