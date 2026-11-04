//! Private, session-only backing for exact settled protocol streams.
use crate::{
    ProtocolFailure, ReportHandle, RunReport, WorkerCompletion, WorkerFailure, WorkerState,
};
use eml_core::RunMode;
use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom, Write},
    process::ExitStatus,
    sync::{Arc, Mutex},
};

/// A settled report whose backing file has no filesystem name.
///
/// Call `write` and `load` on a background thread. The owner must obtain consent
/// before writing diagnostic bytes, bound the number of retained handles and
/// retain the original in-memory report until `write` succeeds. Dropping the last
/// clone releases the backing file; this is neither durable history nor recovery.
#[derive(Clone)]
pub struct StoredReport(Arc<Backing>);
struct Backing {
    file: Mutex<File>,
    bytes: usize,
    mode: RunMode,
    state: WorkerState,
    exit: ExitStatus,
    failure: Option<WorkerFailure>,
    stderr: Vec<u8>,
    stderr_truncated: bool,
    facts: ReplayFacts,
}
#[derive(PartialEq, Eq)]
struct ReplayFacts {
    started: bool,
    items: usize,
    summary: bool,
    failure: Option<ProtocolFailure>,
    unreported: Option<u32>,
}
impl ReplayFacts {
    fn new(report: &RunReport) -> Self {
        Self {
            started: report.start().is_some(),
            items: report.items().len(),
            summary: report.summary().is_some(),
            failure: report.failure(),
            unreported: report.unreported_item_count(),
        }
    }
}
impl StoredReport {
    /// Save exact stdout after proving it reconstructs the retained observations.
    ///
    /// The empty temporary file is explicitly unlinked before any report data is
    /// written. Unlink, write, sync or replay errors leave the caller's original
    /// completion untouched. Replay temporarily holds one additional parsed report.
    pub fn write(completion: &WorkerCompletion, mode: RunMode) -> io::Result<Self> {
        if completion.jsonl.len() >= eml_protocol::MAX_RUN_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "The report exceeds the session stream bound.",
            ));
        }
        let replay =
            ReportHandle::replay(&completion.jsonl, mode == RunMode::DryRun, completion.exit);
        let facts = completion.report.with_settled_report(|original| {
            replay.with_settled_report(|restored| {
                if !same_observations(original, restored)? {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "The captured stream cannot reconstruct this report; it remains in memory.",
                    ));
                }
                Ok(ReplayFacts::new(original))
            })?
        })??;
        drop(replay);
        let (mut file, path) = tempfile::Builder::new()
            .prefix("eml-session-report-")
            .tempfile()?
            .into_parts();
        // Unlike tempfile's unnamed-file fallback, do not ignore unlink failure.
        // A leftover name can contain at most the empty file created above.
        path.close()?;
        file.write_all(&completion.jsonl)?;
        file.sync_all()?;
        Ok(Self(Arc::new(Backing {
            file: Mutex::new(file),
            bytes: completion.jsonl.len(),
            mode,
            state: completion.state,
            exit: completion.exit,
            failure: completion.failure,
            stderr: completion.stderr.clone(),
            stderr_truncated: completion.stderr_truncated,
            facts,
        })))
    }
    /// Restore the exact stream and accepted facts, retaining actual process facts.
    /// Clones share the immutable backing; this lock serializes their file cursors.
    pub fn load(&self) -> io::Result<WorkerCompletion> {
        let mut jsonl = vec![0; self.0.bytes];
        {
            let mut file = self
                .0
                .file
                .lock()
                .map_err(|_| io::Error::other("The session report storage is unavailable."))?;
            file.seek(SeekFrom::Start(0))?;
            file.read_exact(&mut jsonl)?;
        }
        let report = ReportHandle::replay(&jsonl, self.0.mode == RunMode::DryRun, self.0.exit);
        if report.with_settled_report(ReplayFacts::new)? != self.0.facts {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "The stored report could not be reconstructed.",
            ));
        }
        Ok(WorkerCompletion {
            state: self.0.state,
            exit: self.0.exit,
            failure: self.0.failure,
            report,
            jsonl: jsonl.into(),
            stderr: self.0.stderr.clone(),
            stderr_truncated: self.0.stderr_truncated,
        })
    }
    /// Original terminal state, available without loading report bytes.
    pub fn state(&self) -> WorkerState {
        self.0.state
    }
    /// Original transport/worker failure, available without loading report bytes.
    pub fn failure(&self) -> Option<WorkerFailure> {
        self.0.failure
    }
}
fn same_observations(original: &RunReport, restored: &RunReport) -> io::Result<bool> {
    // Compare independently accepted facts before releasing them. In particular,
    // a panicked stdout reader can return no raw bytes despite earlier accepted
    // records; those observations must not disappear through a storage round trip.
    if ReplayFacts::new(original) != ReplayFacts::new(restored)
        || !same_record(&original.start(), &restored.start())?
        || !same_record(&original.summary(), &restored.summary())?
    {
        return Ok(false);
    }
    for (left, right) in original.items().iter().zip(restored.items()) {
        if !same_record(left, right)? {
            return Ok(false);
        }
    }
    Ok(true)
}
fn same_record<T: serde::Serialize>(left: &T, right: &T) -> io::Result<bool> {
    // Only one bounded record is serialized at a time, not a second complete stream.
    Ok(serde_json::to_vec(left)? == serde_json::to_vec(right)?)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::{fs::MetadataExt, process::ExitStatusExt};

    #[test]
    fn stored_report_is_unlinked_private_and_close_on_exec() {
        let exit = ExitStatus::from_raw(2 << 8);
        let completion = WorkerCompletion {
            state: WorkerState::RequestRefused,
            exit,
            failure: None,
            report: ReportHandle::replay(&[], false, exit),
            jsonl: Vec::new().into(),
            stderr: b"refused".to_vec(),
            stderr_truncated: false,
        };
        let stored = StoredReport::write(&completion, RunMode::Create).unwrap();
        let file = stored.0.file.lock().unwrap();
        let metadata = file.metadata().unwrap();
        assert_eq!(metadata.nlink(), 0, "diagnostics must have no pathname");
        assert_eq!(metadata.mode() & 0o777, 0o600);
        assert!(
            rustix::io::fcntl_getfd(&*file)
                .unwrap()
                .contains(rustix::io::FdFlags::CLOEXEC)
        );
    }
}
