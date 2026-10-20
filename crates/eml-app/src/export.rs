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
/// Advisory inspection of the exact proposed report filename. It neither reserves
/// the name nor establishes writability; publication still uses atomic no-replace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReportNameAvailability {
    /// No final entry was observed at this instant.
    Unused,
    /// An entry exists, including a directory or a dangling symlink.
    Occupied,
}
fn validate_report_path(destination: &Path) -> io::Result<()> {
    if !destination.is_absolute() || destination.file_name().is_none() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Choose an absolute report filename",
        ));
    }
    Ok(())
}
/// Inspect a proposed name without following its final symlink. Unknown failures
/// are errors, never evidence that the name is unused.
pub fn inspect_report_destination(destination: &Path) -> io::Result<ReportNameAvailability> {
    validate_report_path(destination)?;
    match std::fs::symlink_metadata(destination) {
        Ok(_) => Ok(ReportNameAvailability::Occupied),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(ReportNameAvailability::Unused),
        Err(error) => Err(error),
    }
}
/// Operation responsible for an actual report-export failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReportExportStage {
    /// Destination binding or namespace admission.
    Destination,
    /// Accepted report serialization.
    Serialization,
    /// Private candidate creation, storage or file synchronization.
    Staging,
    /// Cancellation or address qualification before publication.
    BeforePublication,
    /// Atomic no-replace publication.
    Publication,
    /// Final file identity or byte comparison.
    Verification,
    /// Owned staging removal.
    Cleanup,
    /// Destination namespace synchronization.
    NamespaceSync,
    /// Requested destination address qualification.
    Address,
}
/// Bounded diagnostic with typed operation and native cause. Detail is retained
/// for technical inspection, not parsed to make UI or publication decisions.
#[derive(Clone, Debug)]
pub struct ReportExportFailure {
    /// Actual failed operation.
    pub stage: ReportExportStage,
    /// Native error category where one was observed.
    pub kind: Option<io::ErrorKind>,
    /// Exact native error number where available.
    pub native_code: Option<i32>,
    /// Bounded original diagnostic.
    pub detail: String,
}
impl ReportExportFailure {
    fn native(stage: ReportExportStage, error: &io::Error) -> Self {
        Self {
            stage,
            kind: Some(error.kind()),
            native_code: error.raw_os_error(),
            detail: error.to_string().chars().take(1000).collect(),
        }
    }
    fn binding(stage: ReportExportStage, error: eml_fs::BoundFileError) -> Self {
        let mut failure = Self::invariant(stage, &error);
        if let eml_fs::BoundFileError::ObservationFailed(native) = error {
            failure.kind = Some(native.kind());
            failure.native_code = native.raw_os_error();
        }
        failure
    }
    fn invariant(stage: ReportExportStage, detail: impl std::fmt::Display) -> Self {
        Self {
            stage,
            kind: None,
            native_code: None,
            detail: detail.to_string().chars().take(1000).collect(),
        }
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
    /// Bounded primary and subsequent typed failures.
    pub failures: Vec<ReportExportFailure>,
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
    fn fail(&mut self, failure: ReportExportFailure) {
        if self.failures.len() < 8 {
            self.failures.push(failure);
        }
    }
    /// Complete observed report at the requested address. Inspect synchronization
    /// separately: an unsupported stronger method is not full durability.
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
fn prepare_export(
    report: &TerminalReport,
    destination: &Path,
    control: &RunControl,
) -> Result<(BoundDirectory, Vec<u8>), ReportExportFailure> {
    use ReportExportStage::{BeforePublication, Destination, Serialization};
    checkpoint(control).map_err(|e| ReportExportFailure::native(BeforePublication, &e))?;
    validate_report_path(destination).map_err(|e| ReportExportFailure::native(Destination, &e))?;
    let parent_path = destination.parent().expect("validated absolute filename");
    let parent = BoundDirectory::open(parent_path)
        .map_err(|e| ReportExportFailure::native(Destination, &e))?;
    parent
        .admit_namespace()
        .map_err(|e| ReportExportFailure::native(Destination, &e))?;
    let bytes = report
        .report
        .export_terminal_bytes(report.id, &report.completion)
        .map_err(|e| ReportExportFailure::native(Serialization, &e))?;
    checkpoint(control).map_err(|e| ReportExportFailure::native(BeforePublication, &e))?;
    Ok((parent, bytes))
}
/// Run only on an explicitly owned background task. Existing final entries are
/// never replaced, including original messages. Advisory inspection grants no
/// publication authority; the held no-replace edge remains authoritative.
pub fn export_report(
    report: &TerminalReport,
    destination: &Path,
    control: &RunControl,
) -> ReportExportOutcome {
    let mut result = ReportExportOutcome::empty();
    let (parent, bytes) = match prepare_export(report, destination, control) {
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
            result.fail(ReportExportFailure::native(
                ReportExportStage::Staging,
                &error.error,
            ));
            result.cleanup = error.cleanup;
            return result;
        }
    };
    result.file_sync = Some(candidate.file_sync());
    result.candidate_identity = Some(candidate.identity());
    let before_edge = checkpoint(control).and_then(|()| {
        if parent.check_requested_address()? {
            Ok(())
        } else {
            Err(io::Error::other("Report destination address changed"))
        }
    });
    if let Err(error) = before_edge {
        result.fail(ReportExportFailure::native(
            ReportExportStage::BeforePublication,
            &error,
        ));
        result.cleanup = candidate.cleanup();
        return result;
    }
    publish_export(
        &parent,
        candidate,
        destination.file_name().expect("validated report filename"),
        &bytes,
        &mut result,
    );
    result
}
fn reopen_published(
    parent: &BoundDirectory,
    name: &std::ffi::OsStr,
    identity: Identity,
    result: &mut ReportExportOutcome,
) -> Option<BoundFile> {
    if result.publication == PublicationState::NotPublished {
        return None;
    }
    match parent.open_file(name, false) {
        Ok(file) if file.initial_signature().identity == identity => {
            if result.publication == PublicationState::Unknown {
                result.publication = PublicationState::Published;
            }
            Some(file)
        }
        Ok(_) => {
            result.final_file_verified = Some(false);
            result.fail(ReportExportFailure::invariant(
                ReportExportStage::Verification,
                "Final report identity differs",
            ));
            None
        }
        Err(error) => {
            result.fail(ReportExportFailure::binding(
                ReportExportStage::Verification,
                error,
            ));
            None
        }
    }
}
fn publish_export(
    parent: &BoundDirectory,
    candidate: eml_fs::ReadyCandidate,
    name: &std::ffi::OsStr,
    bytes: &[u8],
    result: &mut ReportExportOutcome,
) {
    let identity = candidate.identity();
    let attempt = candidate.publish(name);
    result.publication = attempt.state;
    let collision = attempt.is_name_collision();
    if let Some(error) = &attempt.error {
        let mut failure = ReportExportFailure::native(ReportExportStage::Publication, error);
        if collision {
            failure.kind = Some(io::ErrorKind::AlreadyExists);
        }
        result.fail(failure);
    }
    // Pin the reopened exact file before candidate cleanup, retaining it through
    // namespace sync and readback. Numeric identity alone is not a lifetime pin.
    let final_file = reopen_published(parent, name, identity, result);
    // No cancellation after an attempted edge: never roll back or skip cleanup.
    result.cleanup = attempt.candidate.cleanup();
    if result.cleanup.state != CleanupState::Complete || result.cleanup.native_code.is_some() {
        result.fail(ReportExportFailure::invariant(
            ReportExportStage::Cleanup,
            "Report staging cleanup is incomplete or unverified",
        ));
    }
    if result.publication != PublicationState::NotPublished {
        qualify_export(parent, final_file.as_ref(), bytes, result);
    }
}
fn qualify_export(
    parent: &BoundDirectory,
    file: Option<&BoundFile>,
    bytes: &[u8],
    result: &mut ReportExportOutcome,
) {
    result.namespace_sync = Some(parent.sync_namespace());
    if result
        .namespace_sync
        .is_some_and(|sync| sync.state == SyncState::Failed)
    {
        result.fail(ReportExportFailure::invariant(
            ReportExportStage::NamespaceSync,
            "Report namespace synchronization failed",
        ));
    }
    if let Some(file) = file {
        match verify_final(file, bytes) {
            Ok(value) => {
                result.final_file_verified = Some(value);
                if !value {
                    result.fail(ReportExportFailure::invariant(
                        ReportExportStage::Verification,
                        "Final report identity or bytes differ",
                    ));
                }
            }
            Err(error) => result.fail(error),
        }
    }
    match parent.check_requested_address() {
        Ok(value) => {
            result.address_verified = Some(value);
            if !value {
                result.fail(ReportExportFailure::invariant(
                    ReportExportStage::Address,
                    "Report destination address changed",
                ));
            }
        }
        Err(error) => result.fail(ReportExportFailure::native(
            ReportExportStage::Address,
            &error,
        )),
    }
}

fn verify_final(file: &BoundFile, bytes: &[u8]) -> Result<bool, ReportExportFailure> {
    let signature = file.initial_signature();
    if signature.size != bytes.len() as u64 {
        return Ok(false);
    }
    file.check_binding()
        .map_err(|e| ReportExportFailure::binding(ReportExportStage::Verification, e))?;
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
                Err(error) => {
                    return Err(ReportExportFailure::native(
                        ReportExportStage::Verification,
                        &error,
                    ));
                }
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
            Err(error) => {
                return Err(ReportExportFailure::native(
                    ReportExportStage::Verification,
                    &error,
                ));
            }
        }
    }
    file.check_binding()
        .map_err(|e| ReportExportFailure::binding(ReportExportStage::Verification, e))?;
    Ok(true)
}
