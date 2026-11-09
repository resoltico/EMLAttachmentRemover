//! One in-process core run: typed observations to the UI, protocol bytes to export.
use eml_core::{
    Event, EventSink, InvalidRequest, OperationFailure, RunControl, RunOutcome, RunRequest,
    RunStart, SettledItem,
};
use eml_protocol::{BoundedEventBuffer, JsonSink, MAX_RUN_BYTES};
use std::{
    cell::RefCell,
    io,
    rc::Rc,
    sync::{Arc, mpsc},
    thread::{self, JoinHandle},
};

pub(super) type Runner = dyn Fn(&RunRequest, &RunControl, &mut RunSink) -> Result<RunOutcome, InvalidRequest>
    + Send
    + Sync;
/// The controller's runner receives core events through this checked sink.
/// Typed observations and protocol serialization are independent consumers;
/// report delivery failure cannot erase an observed publication.
pub struct RunSink {
    sender: mpsc::Sender<Observation>,
    json: JsonSink<BoundedEventBuffer>,
}
enum Observation {
    Start(RunStart),
    Item(Box<SettledItem>),
    Summary(RunOutcome),
}
impl EventSink for RunSink {
    fn emit(&mut self, event: Event<'_>) -> io::Result<()> {
        let observation = match &event {
            Event::Start(s) => Observation::Start(**s),
            Event::Item(i) => Observation::Item(Box::new(SettledItem::from(*i))),
            Event::Summary(s) => Observation::Summary((**s).clone()),
        };
        self.sender
            .send(observation)
            .map_err(|_| io::Error::from(io::ErrorKind::BrokenPipe))?;
        self.json.emit(event)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WorkerState {
    Completed,
    Stopped,
    Rejected,
    Incomplete,
    Panicked,
}
#[derive(Default)]
pub(super) struct RunReport {
    start: Option<RunStart>,
    items: Vec<SettledItem>,
    summary: Option<RunOutcome>,
    settled: bool,
}
impl RunReport {
    pub(super) fn start(&self) -> Option<&RunStart> {
        self.start.as_ref()
    }
    pub(super) fn items(&self) -> &[SettledItem] {
        &self.items
    }
    pub(super) fn summary(&self) -> Option<&RunOutcome> {
        self.summary.as_ref()
    }
    pub(super) fn source_path(&self, position: usize) -> Option<std::path::PathBuf> {
        Some(self.items.get(position)?.source.clone())
    }
    pub(super) fn reveal_path(&self, position: usize) -> Option<std::path::PathBuf> {
        self.items.get(position)?.reveal_path().map(PathBuf::from)
    }
    pub(super) fn settled_totals(&self) -> Option<eml_core::vocabulary::BatchTotals> {
        Some(self.summary.as_ref()?.settled_totals(&self.items))
    }
    pub(super) fn can_offer_copy(&self) -> bool {
        eml_core::vocabulary::can_offer_copy(
            self.items.iter().map(SettledItem::outcome),
            self.summary
                .as_ref()
                .and_then(RunOutcome::run_error)
                .map(OperationFailure::code),
        )
    }
    pub(super) fn results_cover_inventory(&self) -> bool {
        self.settled
            && self
                .summary
                .as_ref()
                .is_some_and(RunOutcome::results_cover_inventory)
    }
}
use std::path::PathBuf;
/// UI-owned facts; the worker never borrows this cell or a Slint object.
#[derive(Clone, Default)]
pub(super) struct ReportHandle(Rc<RefCell<RunReport>>);
impl ReportHandle {
    pub(super) fn with_report<T>(&self, render: impl FnOnce(&RunReport) -> T) -> T {
        render(&self.0.borrow())
    }
}
#[derive(Clone)]
pub(super) struct WorkerCompletion {
    pub state: WorkerState,
    pub failure: Option<OperationFailure>,
    pub report: ReportHandle,
    pub jsonl: Arc<[u8]>,
}
impl WorkerCompletion {
    pub(super) fn rejected(failure: OperationFailure) -> Self {
        let report = ReportHandle::default();
        report.0.borrow_mut().settled = true;
        Self {
            state: WorkerState::Rejected,
            failure: Some(failure),
            report,
            jsonl: Arc::from([]),
        }
    }
}
struct Finished {
    outcome: Option<RunOutcome>,
    failure: Option<OperationFailure>,
    state: WorkerState,
    jsonl: Arc<[u8]>,
}
pub(super) struct WorkerRun {
    control: RunControl,
    stopped: bool,
    receiver: mpsc::Receiver<Observation>,
    task: Option<JoinHandle<Finished>>,
    report: ReportHandle,
}
impl WorkerRun {
    pub(super) fn start(runner: Arc<Runner>, request: RunRequest) -> io::Result<Self> {
        let control = RunControl::default();
        let run_control = control.clone();
        let (sender, receiver) = mpsc::channel();
        let name = if request.mode == eml_core::RunMode::DryRun {
            "eml-check"
        } else {
            "eml-create"
        };
        let task = thread::Builder::new().name(name.into()).spawn(move || {
            let mut sink = RunSink {
                sender,
                json: JsonSink::new(BoundedEventBuffer::new(MAX_RUN_BYTES - 1)),
            };
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                runner(&request, &run_control, &mut sink)
            }));
            let (outcome, failure, state) = match result {
                Ok(Ok(outcome)) => {
                    let failure = outcome
                        .delivery_failure()
                        .map(eml_core::DeliveryFailure::operation_failure);
                    let state = if failure.is_some() {
                        WorkerState::Incomplete
                    } else if outcome.interrupted() {
                        WorkerState::Stopped
                    } else {
                        WorkerState::Completed
                    };
                    (Some(outcome), failure, state)
                }
                Ok(Err(error)) => (None, Some(error.into_failure()), WorkerState::Rejected),
                Err(_) => (None, None, WorkerState::Panicked),
            };
            Finished {
                outcome,
                failure,
                state,
                jsonl: sink.json.into_inner().into_bytes().into(),
            }
        })?;
        Ok(Self {
            control,
            stopped: false,
            receiver,
            task: Some(task),
            report: ReportHandle::default(),
        })
    }
    pub(super) fn report(&self) -> &ReportHandle {
        &self.report
    }
    pub(super) fn stop(&mut self) {
        if !self.stopped {
            self.stopped = true;
            self.control.request_stop();
        }
    }
    pub(super) fn poll(&mut self) -> Option<WorkerCompletion> {
        self.drain();
        if !self.task.as_ref().is_some_and(JoinHandle::is_finished) {
            return None;
        }
        let finished = self
            .task
            .take()
            .unwrap()
            .join()
            .unwrap_or_else(|_| Finished {
                outcome: None,
                failure: None,
                state: WorkerState::Panicked,
                jsonl: Arc::from([]),
            });
        self.drain();
        let mut report = self.report.0.borrow_mut();
        if let Some(outcome) = finished.outcome {
            report.summary = Some(outcome);
        }
        report.settled = true;
        drop(report);
        Some(WorkerCompletion {
            state: finished.state,
            failure: finished.failure,
            report: self.report.clone(),
            jsonl: finished.jsonl,
        })
    }
    fn drain(&self) {
        let mut report = self.report.0.borrow_mut();
        for observation in self.receiver.try_iter() {
            match observation {
                Observation::Start(s) => report.start = Some(s),
                Observation::Item(i) => report.items.push(*i),
                Observation::Summary(s) => report.summary = Some(s),
            }
        }
    }
}
impl Drop for WorkerRun {
    fn drop(&mut self) {
        if self.task.is_some() {
            self.stop();
        }
    }
}

#[cfg(test)]
#[path = "../tests/worker/projection.rs"]
mod tests;
