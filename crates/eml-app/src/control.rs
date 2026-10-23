//! Monotonic desktop intent, independent of native PID or report backpressure.
use crate::RunId;
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

/// A poisoned control lock closes admission rather than resetting intent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ControlError;
impl std::fmt::Display for ControlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Worker control state is unavailable; stop further admission")
    }
}
impl std::error::Error for ControlError {}
#[derive(Default)]
struct State {
    aborted: bool,
    stops: u8,
    feeding: bool,
    reaped: bool,
    shutdown: Option<Instant>,
}
/// Shared intent for exactly one admitted run. It never owns a PID or performs I/O.
#[derive(Clone)]
pub struct WorkerControl {
    id: RunId,
    operation_id: eml_protocol::desktop::OperationId,
    state: Arc<Mutex<State>>,
}
/// Desired control action; successful OS signal delivery is a separate observation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkerIntent {
    /// Integrity/transport failure has closed further feeding independently of UI Stop.
    pub aborted: bool,
    /// Monotonic number of requested interrupts, capped at two.
    pub interrupts: u8,
    /// Feeding was granted; physical delivery may have started and cannot be denied.
    pub input_may_have_been_sent: bool,
    /// Actual terminal reap has been observed by the native owner.
    pub reaped: bool,
    /// Confirmed shutdown has closed admission for this run.
    pub closing: bool,
    /// Final native Kill is due after the fixed cooperative/forced Quit allowances.
    pub kill_due: bool,
}
impl WorkerControl {
    /// Bind a fresh control reference to the immutable queue identity.
    pub fn new(id: RunId) -> Self {
        Self::for_operation(
            eml_protocol::desktop::OperationId::new(id.get()).expect("nonzero run identity"),
            id,
        )
    }
    /// Bind the exact child operation independently of its source report.
    pub fn for_operation(operation_id: eml_protocol::desktop::OperationId, id: RunId) -> Self {
        Self {
            id,
            operation_id,
            state: Arc::new(Mutex::new(State::default())),
        }
    }
    /// Exact child operation identity, independent of the report identity.
    pub fn operation_id(&self) -> eml_protocol::desktop::OperationId {
        self.operation_id
    }
    /// Stable run identity; stale callbacks must not control another run.
    pub fn id(&self) -> RunId {
        self.id
    }
    /// Record Stop or the confirmed second interrupt before asynchronous notification.
    /// A late request after actual reap has no effect on observed outcomes.
    pub fn request_stop(&self) -> Result<bool, ControlError> {
        let mut state = self.state.lock().map_err(|_| ControlError)?;
        if state.reaped || state.stops == 2 {
            return Ok(false);
        }
        state.stops += 1;
        Ok(true)
    }
    /// Atomically grant the sole feed attempt only while admission remains open.
    /// This precedes the physical pipe write and deliberately makes no delivery claim.
    pub fn begin_feeding(&self) -> Result<bool, ControlError> {
        let mut state = self.state.lock().map_err(|_| ControlError)?;
        if state.reaped
            || state.aborted
            || state.feeding
            || state.stops > 0
            || state.shutdown.is_some()
        {
            return Ok(false);
        }
        state.feeding = true;
        Ok(true)
    }
    /// Record confirmed Quit once; repeated requests cannot extend its deadlines.
    pub fn confirm_shutdown(&self, now: Instant) -> Result<(), ControlError> {
        let mut state = self.state.lock().map_err(|_| ControlError)?;
        if state.shutdown.is_none() {
            state.shutdown = Some(now);
        }
        if !state.reaped {
            state.stops = state.stops.max(1);
        }
        Ok(())
    }
    /// Record actual native terminal reap. Pipe/model completion remains separate.
    pub fn process_reaped(&self) -> Result<(), ControlError> {
        self.state.lock().map_err(|_| ControlError)?.reaped = true;
        Ok(())
    }
    #[cfg(target_os = "macos")]
    pub(crate) fn abort_transport(&self) -> Result<(), ControlError> {
        self.state.lock().map_err(|_| ControlError)?.aborted = true;
        Ok(())
    }
    /// Observe intent and latch due escalation without holding any report,
    /// native-owner, pipe or UI lock.
    pub fn intent(&self, now: Instant) -> Result<WorkerIntent, ControlError> {
        let mut state = self.state.lock().map_err(|_| ControlError)?;
        let elapsed = state
            .shutdown
            .map(|start| now.saturating_duration_since(start));
        if !state.reaped && elapsed.is_some_and(|elapsed| elapsed >= Duration::from_secs(10)) {
            state.stops = 2;
        }
        Ok(WorkerIntent {
            aborted: state.aborted,
            interrupts: state.stops,
            input_may_have_been_sent: state.feeding,
            reaped: state.reaped,
            closing: state.shutdown.is_some(),
            kill_due: !state.reaped
                && elapsed.is_some_and(|elapsed| elapsed >= Duration::from_secs(12)),
        })
    }
}
