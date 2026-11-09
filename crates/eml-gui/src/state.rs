//! One batch's pure transitions. Worker ownership lives in the controller.
use crate::worker::{ReportHandle, WorkerCompletion};
use std::path::PathBuf;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Check,
    Create,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum State {
    Empty,
    Checking,
    Ready,
    Removing,
    Stopped,
    Done,
}
impl From<State> for crate::BatchState {
    fn from(state: State) -> Self {
        match state {
            State::Empty => Self::Empty,
            State::Checking => Self::Checking,
            State::Ready => Self::Ready,
            State::Removing => Self::Removing,
            State::Stopped => Self::Stopped,
            State::Done => Self::Done,
        }
    }
}
pub(super) struct Slot {
    pub generation: u64,
    pub report: Option<ReportHandle>,
    pub completion: Option<WorkerCompletion>,
    pub launch_error: bool,
    pub stopped: bool,
}
impl Slot {
    pub(super) fn settled(&self) -> bool {
        self.completion.is_some() || self.launch_error
    }
}
#[derive(Default)]
pub(super) struct Batch {
    pub inputs: Vec<PathBuf>,
    pub destination: Option<PathBuf>,
    pub check: Option<Slot>,
    pub create: Option<Slot>,
    pub previous_create: Option<ReportHandle>,
    pub active: Option<(Kind, u64)>,
    pub pending: Option<Kind>,
    pub problems_only: bool,
    pub selected: Option<PathBuf>,
    sequence: u64,
    revision: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Effect {
    InterruptCheck,
    BusyAdding,
}
impl Batch {
    pub(super) fn revision(&self) -> u64 {
        self.revision
    }
    pub(super) fn touch(&mut self) {
        self.revision = self
            .revision
            .checked_add(1)
            .expect(crate::wording::REVISION_EXHAUSTED);
    }
    pub(super) fn filter(&mut self, enabled: bool) {
        if self.problems_only != enabled {
            self.problems_only = enabled;
            self.touch();
        }
    }
    pub(super) fn select(&mut self, path: Option<PathBuf>) {
        if self.selected != path {
            self.selected = path;
            self.touch();
        }
    }
    pub(super) fn state(&self) -> State {
        if self.inputs.is_empty() {
            State::Empty
        } else if matches!(self.active, Some((Kind::Create, _))) {
            State::Removing
        } else if self.create.as_ref().is_some_and(Slot::settled) {
            if self.create.as_ref().is_some_and(|slot| slot.stopped) {
                State::Stopped
            } else {
                State::Done
            }
        } else if self.pending.is_some()
            || matches!(self.active, Some((Kind::Check, _)))
            || !self.check.as_ref().is_some_and(Slot::settled)
        {
            State::Checking
        } else {
            State::Ready
        }
    }
    pub(super) fn slot(&self, kind: Kind) -> Option<&Slot> {
        match kind {
            Kind::Check => self.check.as_ref(),
            Kind::Create => self.create.as_ref(),
        }
    }
    pub(super) fn slot_mut(&mut self, kind: Kind) -> Option<&mut Slot> {
        match kind {
            Kind::Check => self.check.as_mut(),
            Kind::Create => self.create.as_mut(),
        }
    }
    pub(super) fn add(&mut self, paths: Vec<PathBuf>) -> Option<Effect> {
        if self.state() == State::Removing {
            return Some(Effect::BusyAdding);
        }
        if matches!(self.state(), State::Done | State::Stopped) {
            self.reset();
        }
        for path in paths {
            if !self.inputs.contains(&path) {
                self.inputs.push(path);
            }
        }
        self.changed()
    }
    pub(super) fn changed(&mut self) -> Option<Effect> {
        self.touch();
        self.check = None;
        self.create = None;
        self.previous_create = None;
        self.problems_only = false;
        self.selected = None;
        self.pending = (!self.inputs.is_empty()).then_some(Kind::Check);
        self.active
            .filter(|(kind, _)| *kind == Kind::Check)
            .map(|_| Effect::InterruptCheck)
    }
    pub(super) fn reset(&mut self) -> Option<Effect> {
        self.inputs.clear();
        self.changed()
    }
    pub(super) fn remove(&mut self) -> Option<Effect> {
        self.touch();
        self.pending = Some(Kind::Create);
        self.active
            .filter(|(kind, _)| *kind == Kind::Check)
            .map(|_| Effect::InterruptCheck)
    }
    pub(super) fn start(&mut self) -> Option<(Kind, u64)> {
        if self.active.is_some() {
            return None;
        }
        let kind = self.pending.take()?;
        self.touch();
        self.sequence = self
            .sequence
            .checked_add(1)
            .expect(crate::wording::GENERATION_EXHAUSTED);
        let generation = self.sequence;
        let slot = Some(Slot {
            generation,
            report: None,
            completion: None,
            launch_error: false,
            stopped: false,
        });
        match kind {
            Kind::Check => self.check = slot,
            Kind::Create => {
                self.previous_create = self.create.take().and_then(|s| s.report);
                self.create = slot;
            }
        }
        self.active = Some((kind, generation));
        self.active
    }
    pub(super) fn settle(
        &mut self,
        kind: Kind,
        generation: u64,
        completion: WorkerCompletion,
        stopped: bool,
    ) {
        if self.active != Some((kind, generation)) {
            return;
        }
        self.touch();
        if kind == Kind::Create {
            self.previous_create = None;
        }
        self.active = None;
        if let Some(slot) = self
            .slot_mut(kind)
            .filter(|slot| slot.generation == generation)
        {
            slot.stopped = stopped;
            slot.completion = Some(completion);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stale_generation_cannot_settle_or_replace_the_current_worker() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join("mail.eml"),
            b"Subject: Plain\r\n\r\nBody\r\n",
        )
        .unwrap();
        let request = eml_core::RunRequest::from_absolute(
            vec![root.path().join("mail.eml")],
            eml_core::OutputLocation::BesideSource,
            eml_core::RunMode::DryRun,
        )
        .unwrap();
        let paths = request.paths.clone();
        let mut worker =
            crate::worker::WorkerRun::start(std::sync::Arc::new(eml_core::run), request).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        let completion = loop {
            if let Some(c) = worker.poll() {
                break c;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        };
        let mut batch = Batch::default();
        batch.add(paths);
        let (kind, generation) = batch.start().unwrap();
        batch.settle(kind, generation + 1, completion.clone(), false);
        assert_eq!(batch.active, Some((kind, generation)));
        assert!(!batch.check.as_ref().unwrap().settled());
        batch.settle(kind, generation, completion.clone(), false);
        assert_eq!(batch.state(), State::Ready);
        batch.changed();
        let (kind, generation) = batch.start().unwrap();
        assert_eq!(batch.reset(), Some(Effect::InterruptCheck));
        batch.settle(kind, generation, completion, false);
        assert_eq!(batch.state(), State::Empty);
        assert!(batch.check.is_none());
        assert!(batch.pending.is_none());
    }
    #[test]
    fn changes_collapse_and_create_waits_for_check_settlement() {
        let mut batch = Batch::default();
        assert_eq!(batch.state(), State::Empty);
        batch.add(vec!["/a.eml".into()]);
        batch.add(vec!["/b.eml".into(), "/a.eml".into()]);
        batch.add(vec!["/c.eml".into()]);
        assert_eq!(batch.inputs.len(), 3);
        assert_eq!(batch.pending, Some(Kind::Check));
        assert_eq!(batch.start(), Some((Kind::Check, 1)));
        assert_eq!(batch.state(), State::Checking);
        assert_eq!(batch.remove(), Some(Effect::InterruptCheck));
        assert_eq!(batch.start(), None);
        batch.active = None;
        assert_eq!(batch.start(), Some((Kind::Create, 2)));
        assert_eq!(batch.state(), State::Removing);
        assert_eq!(
            batch.add(vec!["/refused.eml".into()]),
            Some(Effect::BusyAdding)
        );
        assert_eq!(batch.inputs.len(), 3);
        batch.active = None;
        batch.create.as_mut().unwrap().launch_error = true;
        assert_eq!(batch.state(), State::Done);
        batch.create.as_mut().unwrap().stopped = true;
        assert_eq!(batch.state(), State::Stopped);
        batch.add(vec!["/replacement.eml".into()]);
        assert_eq!(batch.inputs, vec![PathBuf::from("/replacement.eml")]);
        assert!(batch.create.is_none());
        batch.reset();
        assert_eq!(batch.state(), State::Empty);
    }
}

#[derive(Default)]
pub(super) struct ProgressEstimate {
    started: u64,
    sampled: u64,
    completed: u32,
    rate: Option<f64>,
}
impl ProgressEstimate {
    pub(super) fn start(now: u64) -> Self {
        Self {
            started: now,
            sampled: now,
            ..Default::default()
        }
    }
    pub(super) fn observe(&mut self, now: u64, completed: u32) {
        if completed > self.completed && now > self.sampled {
            let rate = f64::from(completed - self.completed) * 1000.0 / (now - self.sampled) as f64;
            self.rate = Some(
                self.rate
                    .map_or(rate, |previous| previous * 0.8 + rate * 0.2),
            );
            self.sampled = now;
            self.completed = completed;
        }
    }
    pub(super) fn remaining(&self, now: u64, total: Option<u32>) -> Option<u64> {
        let total = total?;
        if self.completed < 10 || now.saturating_sub(self.started) < 3000 {
            return None;
        }
        self.rate
            .map(|rate| (f64::from(total.saturating_sub(self.completed)) / rate).ceil() as u64)
    }
}
