//! Explicit metadata export through the same held filesystem primitives as Core.
use crate::{ReportHandle, RunId, WorkerCompletion};
use eml_core::RunControl;
use eml_fs::{
    BoundDirectory, BoundFile, CleanupReceipt, CleanupState, Identity, PublicationState,
    SyncReceipt, SyncState,
};
use std::{io, path::Path};
/// Captured accepted facts and terminal lifecycle for one exact session run.
/// Keeping this alive retains the full report reservation during export.
#[derive(Clone)]
pub struct TerminalReport {
    id: RunId,
    report: ReportHandle,
    completion: WorkerCompletion,
}
impl TerminalReport {
    pub(crate) fn new(
        id: RunId,
        report: ReportHandle,
        completion: WorkerCompletion,
    ) -> Option<Self> {
        completion.settled.then_some(Self {
            id,
            report,
            completion,
        })
    }
    /// Exact session-local run captured before opening the save dialog.
    pub fn id(&self) -> RunId {
        self.id
    }
}
/// Independent observations from one user-requested report export.
#[derive(Debug)]
pub struct ReportExportOutcome {
    /// Actual no-replace publication observation.
    pub publication: PublicationState,
    /// Actual candidate file synchronization, when staging reached it.
    pub file_sync: Option<SyncReceipt>,
    /// Actual held destination directory synchronization, when requested.
    pub namespace_sync: Option<SyncReceipt>,
    /// Requested directory address still denotes the held directory.
    pub address_verified: Option<bool>,
    /// Exact held candidate identity, when staging succeeded.
    pub candidate_identity: Option<Identity>,
    /// Reopened final entry has this identity and exact serialized bytes,
    /// with stable regular-file binding throughout the readback.
    pub final_file_verified: Option<bool>,
    /// Actual owned staging cleanup; never a final-file rollback.
    pub cleanup: CleanupReceipt,
    /// Bounded primary and subsequent native/serialization failures.
    pub failures: Vec<String>,
}
impl ReportExportOutcome {
    fn empty() -> Self {
        Self {
            publication: PublicationState::NotPublished,
            file_sync: None,
            namespace_sync: None,
            address_verified: None,
            final_file_verified: None,
            candidate_identity: None,
            cleanup: CleanupReceipt {
                state: CleanupState::NotNeeded,
                residual_name: None,
                native_code: None,
            },
            failures: Vec::new(),
        }
    }
    fn fail(&mut self, error: impl std::fmt::Display) {
        if self.failures.len() < 8 {
            self.failures
                .push(error.to_string().chars().take(1000).collect());
        }
    }
    /// Complete observed report at the requested address. Inspect synchronization
    /// receipts separately: an unsupported stronger method is not full durability.
    pub fn usable(&self) -> bool {
        self.publication == PublicationState::Published
            && self.address_verified == Some(true)
            && self.final_file_verified == Some(true)
            && self.cleanup.state == CleanupState::Complete
            && self
                .file_sync
                .is_some_and(|sync| sync.state != SyncState::Failed)
            && self
                .namespace_sync
                .is_some_and(|sync| sync.state != SyncState::Failed)
            && self.failures.is_empty()
    }
}
fn checkpoint(control: &RunControl) -> io::Result<()> {
    if control.is_stopped() {
        Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "Report export cancelled",
        ))
    } else {
        Ok(())
    }
}
/// Run only on an explicitly owned background task. The caller retains the
/// report handle until this function, its serialized buffer and task have ended.
/// Existing final entries are never replaced, including original message files.
pub fn export_report(
    report: &TerminalReport,
    destination: &Path,
    control: &RunControl,
) -> ReportExportOutcome {
    let mut result = ReportExportOutcome::empty();
    let prepare = || -> Result<(BoundDirectory, Vec<u8>), String> {
        checkpoint(control).map_err(|error| error.to_string())?;
        if !destination.is_absolute() || destination.file_name().is_none() {
            return Err("Choose an absolute report filename".into());
        }
        let parent =
            BoundDirectory::open(destination.parent().ok_or("Report parent is unavailable")?)
                .map_err(|error| error.to_string())?;
        parent
            .admit_namespace()
            .map_err(|error| error.to_string())?;
        let bytes = report
            .report
            .export_terminal_bytes(report.id, &report.completion)
            .map_err(|error| error.to_string())?;
        checkpoint(control).map_err(|error| error.to_string())?;
        Ok((parent, bytes))
    };
    let (parent, bytes) = match prepare() {
        Ok(value) => value,
        Err(error) => {
            result.fail(error);
            return result;
        }
    };
    let candidate = match parent.stage_bytes(&bytes, || checkpoint(control)) {
        Ok(candidate) => candidate,
        Err(error) => {
            result.file_sync = error.file_sync;
            result.fail(&error.error);
            result.cleanup = error.cleanup;
            return result;
        }
    };
    result.file_sync = Some(candidate.file_sync());
    let identity = candidate.identity();
    result.candidate_identity = Some(identity);
    let before_edge = checkpoint(control).and_then(|()| {
        if parent.check_requested_address()? {
            Ok(())
        } else {
            Err(io::Error::other("Report destination address changed"))
        }
    });
    if let Err(error) = before_edge {
        result.fail(error);
        result.cleanup = candidate.cleanup();
        return result;
    }
    let name = destination.file_name().expect("validated report filename");
    let attempt = candidate.publish(name);
    result.publication = attempt.state;
    if let Some(error) = attempt.error {
        result.fail(error);
    }
    // Open the final entry while the candidate still pins its identity. Keep
    // this exact file alive through cleanup, sync and readback: a saved numeric
    // identity alone is not an object lifetime guarantee.
    let final_file = if result.publication != PublicationState::NotPublished {
        match parent.open_file(name, false) {
            Ok(file) if file.initial_signature().identity == identity => {
                if result.publication == PublicationState::Unknown {
                    result.publication = PublicationState::Published;
                }
                Some(file)
            }
            Ok(_) => {
                result.final_file_verified = Some(false);
                result.fail("Final report identity differs");
                None
            }
            Err(error) => {
                result.fail(error);
                None
            }
        }
    } else {
        None
    };
    // After an attempted edge, cancellation never rolls back or skips cleanup.
    result.cleanup = attempt.candidate.cleanup();
    if result.cleanup.state != CleanupState::Complete || result.cleanup.native_code.is_some() {
        result.fail("Report staging cleanup is incomplete or unverified");
    }
    if result.publication != PublicationState::NotPublished {
        // This sync covers both the publication and owned-stage removal edges.
        result.namespace_sync = Some(parent.sync_namespace());
        if result
            .namespace_sync
            .is_some_and(|sync| sync.state == SyncState::Failed)
        {
            result.fail("Report namespace synchronization failed");
        }
        if let Some(file) = &final_file {
            match verify_final(file, &bytes) {
                Ok(value) => {
                    result.final_file_verified = Some(value);
                    if !value {
                        result.fail("Final report identity or bytes differ");
                    }
                }
                Err(error) => result.fail(error),
            }
        }
        match parent.check_requested_address() {
            Ok(value) => {
                result.address_verified = Some(value);
                if !value {
                    result.fail("Report destination address changed");
                }
            }
            Err(error) => result.fail(error),
        }
    }
    result
}
fn verify_final(file: &BoundFile, bytes: &[u8]) -> io::Result<bool> {
    let signature = file.initial_signature();
    if signature.size != bytes.len() as u64 {
        return Ok(false);
    }
    file.check_binding().map_err(io::Error::other)?;
    let mut buffer = [0u8; 64 * 1024];
    let mut offset = 0;
    for expected in bytes.chunks(buffer.len()) {
        let mut filled = 0;
        while filled < expected.len() {
            match file.read_at(
                &mut buffer[filled..expected.len()],
                (offset + filled) as u64,
            ) {
                Ok(0) => return Ok(false),
                Ok(count) => filled += count,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(error),
            }
        }
        if &buffer[..expected.len()] != expected {
            return Ok(false);
        }
        offset += expected.len();
    }
    let mut tail = [0];
    loop {
        match file.read_at(&mut tail, offset as u64) {
            Ok(0) => break,
            Ok(_) => return Ok(false),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    file.check_binding().map_err(io::Error::other)?;
    Ok(true)
}
