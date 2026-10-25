//! Human-facing explanations shared by the CLI and desktop, outside wire facts.

/// Explain an observed failure and the next useful action. Unknown protocol codes
/// keep their supplied detail; they must not acquire an invented interpretation.
pub fn recovery_hint(code: &str) -> Option<&'static str> {
    use crate::ErrorCode;
    match ErrorCode::from_wire_name(code)? {
        ErrorCode::ReadFailed => Some(
            "Check that the selected file or folder still exists and that you have permission to read it, then retry.",
        ),
        ErrorCode::Malformed => Some(
            "This message's MIME structure is damaged or incomplete. Export it again from your mail app and retry with that copy.",
        ),
        ErrorCode::Protected => Some(
            "Protected content would need to change in the retained message body. Obtain an unprotected export if appropriate.",
        ),
        ErrorCode::UnsupportedMime => {
            Some("This report cannot be converted to a body-only copy. Keep the original report.")
        }
        ErrorCode::NoBody => Some(
            "No supported plain-text or HTML message body would remain. Choose a message that contains a text body.",
        ),
        ErrorCode::InputLimitExceeded => {
            Some("Select fewer messages or a smaller folder and try again.")
        }
        ErrorCode::NoInputs => Some(
            "No .eml messages were found. Select messages or another folder; hidden entries and existing .no-attachments.eml copies are skipped during folder scans.",
        ),
        ErrorCode::OutputOverlap => Some(
            "The copy folder overlaps a selected source folder. Choose a destination outside the selected tree, or save copies beside the originals.",
        ),
        ErrorCode::DestinationCollision => Some(
            "Split colliding messages into separate runs and destination folders so each copy has its own path.",
        ),
        ErrorCode::DestinationExists => Some(
            "Choose another copy folder, or move the existing destination before retrying. Existing entries will not be overwritten.",
        ),
        ErrorCode::StageFailed => Some(
            "Check destination write permissions and available disk space, or choose another copy folder.",
        ),
        _ => None,
    }
}

/// Material consequence of the body-only policy; HTML references are not rewritten.
pub const RELATED_RESOURCE_NOTICE: &str = "Inline images or other related resources were removed. They may appear missing in the copied message.";

/// Consequence for a prepared candidate that has not been confirmed as published.
pub const PREPARED_RELATED_RESOURCE_NOTICE: &str = "The prepared copy omits inline images or other related resources. They may appear missing if that copy is saved.";

/// Terminal result category supplied by the producer, not a capability.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResultKind {
    /// Producer reports complete new-copy qualification.
    Created,
    /// Producer reports complete existing-copy qualification.
    Existing,
    /// Read-only preparation.
    Planned,
    /// A requirement failed.
    Failed,
}
/// Observed publication edge, independent of requested destination spelling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Publication {
    /// No new copy was published.
    NotPublished,
    /// A complete new copy was published.
    Published,
    /// A complete copy might exist.
    Unknown,
}
/// Complete display-safe existing-copy observations.
#[derive(Clone, Debug)]
pub struct ExistingFacts {
    /// A native file identity was established.
    pub identity_known: bool,
    /// Complete comparison result, if attempted.
    pub bytes_equal: Option<bool>,
    /// Before/after signature and binding were unchanged.
    pub stable: bool,
    /// Requested address observation.
    pub address: crate::AddressState,
    /// Actual file synchronization.
    pub file_sync: Option<crate::SyncReceipt>,
    /// Actual namespace synchronization, including edge accounting.
    pub namespace_sync: crate::NamespaceObservation,
}
/// Facts used only for presentation; constructing these grants no I/O authority.
#[derive(Clone, Debug)]
pub struct OutcomeFacts {
    /// Producer's result category.
    pub result: ResultKind,
    /// Exact read-only action, absent outside planning.
    pub plan: Option<crate::PlanAction>,
    /// Independently observed edge effect.
    pub publication: Publication,
    /// Requested address observation.
    pub address: crate::AddressState,
    /// Held candidate/publication identity was established.
    pub candidate_identity_known: bool,
    /// Exact final readback when explicitly supplied by a producer.
    pub final_readback: Option<bool>,
    /// Actual candidate file synchronization.
    pub file_sync: Option<crate::SyncReceipt>,
    /// Actual namespace synchronization, including edge accounting.
    pub namespace_sync: crate::NamespaceObservation,
    /// Complete native cleanup observation; names are display data only.
    pub cleanup: crate::CleanupReceipt,
    /// Existing-copy observations independent of new publication.
    pub existing: Option<ExistingFacts>,
    /// Known diagnostic category. Unknown transport codes remain unknown.
    pub error: Option<crate::ErrorCode>,
    /// Whether the accepted metadata selected related resources.
    pub related_resources: bool,
}
/// Conservative semantic result shared by renderers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeaningKind {
    /// A new candidate is prepared only.
    Prepared,
    /// An existing candidate remains to be qualified.
    Comparison,
    /// A new copy passed all qualification requirements.
    Created,
    /// Existing matching bytes passed all qualification requirements.
    ExistingVerified,
    /// Complete publication occurred but qualification failed.
    PublishedIncomplete,
    /// Existing bytes match but qualification failed.
    ExistingMatched,
    /// A complete copy may exist.
    PublicationUncertain,
    /// Failure before new publication.
    NotPublished,
    /// Missing or contradictory facts cannot establish an outcome.
    Unestablished,
}
/// Display prominence; never influences execution.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    /// Qualified success.
    Success,
    /// Read-only observation.
    Information,
    /// Qualification or certainty is incomplete.
    Attention,
}
/// Meaning of removal metadata for these exact accepted facts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemovalMode {
    /// Candidate decisions, not realized deletion.
    Selected,
    /// Removed from an observed published copy.
    Removed,
    /// Verified absent from existing matching bytes.
    Absent,
}
impl RemovalMode {
    /// Canonical part heading.
    pub const fn parts(self) -> &'static str {
        match self {
            Self::Selected => "Parts selected for removal",
            Self::Removed => "Removed parts",
            Self::Absent => "Parts absent from matching bytes",
        }
    }
    /// Canonical stale-field heading.
    pub const fn fields(self) -> &'static str {
        match self {
            Self::Selected => "Fields selected for removal",
            Self::Removed => "Removed stale fields",
            Self::Absent => "Fields absent from matching bytes",
        }
    }
}
/// Simultaneous consequences; one primary failure cannot erase cleanup facts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Consequence {
    /// Complete publication is known, qualification is incomplete.
    Published,
    /// Missing receipt or ambiguous edge requires inspection.
    PublicationUnknown,
    /// No new copy was published.
    NoPublication,
    /// Matching existing bytes do not imply synchronization.
    ExistingUnqualified,
    /// Requested address is not verified.
    AddressUnqualified,
    /// Required durability operations did not qualify.
    DurabilityUnqualified,
    /// An owned temporary resource remains.
    ResidualCleanup,
    /// Cleanup could not be established.
    UnknownCleanup,
}
impl Consequence {
    /// Canonical explanation and next action, without interpolated paths.
    pub const fn text(self) -> &'static str {
        match self {
            Self::Published => {
                "A complete copy was published; qualification did not complete. Inspect the destination before retrying."
            }
            Self::PublicationUnknown => {
                "A complete copy may exist. Inspect the destination before retrying."
            }
            Self::NoPublication => "No new copy was published.",
            Self::ExistingUnqualified => {
                "Existing bytes matched; synchronization/address qualification did not complete."
            }
            Self::AddressUnqualified => "The requested destination address could not be qualified.",
            Self::DurabilityUnqualified => {
                "Required file or namespace synchronization did not complete successfully."
            }
            Self::ResidualCleanup => {
                "A known owned temporary resource remains. Inspect the cleanup details."
            }
            Self::UnknownCleanup => {
                "Temporary-resource cleanup could not be established. Inspect the cleanup details."
            }
        }
    }
}
/// Single shared interpretation; raw facts remain available in technical reports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutcomeMeaning {
    /// Conservative semantic category.
    pub kind: MeaningKind,
    /// Display prominence.
    pub severity: Severity,
    /// Removal/stale-field heading mode.
    pub removal_mode: RemovalMode,
    /// At most eight simultaneous consequences, in stable display order.
    pub consequences: Vec<Consequence>,
    /// Related-resource notice appropriate to this exact qualification.
    pub related_notice: Option<&'static str>,
}
impl MeaningKind {
    /// Canonical concise result label.
    pub const fn label(self) -> &'static str {
        match self {
            MeaningKind::Prepared => "Prepared only",
            MeaningKind::Comparison => "Existing candidate to verify",
            MeaningKind::Created => "Created",
            MeaningKind::ExistingVerified => "Existing copy verified",
            MeaningKind::PublishedIncomplete => "Complete copy published; qualification incomplete",
            MeaningKind::ExistingMatched => "Existing bytes matched",
            MeaningKind::PublicationUncertain => "Publication uncertain",
            MeaningKind::NotPublished => "No new copy published",
            MeaningKind::Unestablished => "Outcome cannot be established",
        }
    }
}
impl OutcomeMeaning {
    /// Canonical concise result label.
    pub const fn label(&self) -> &'static str {
        self.kind.label()
    }
}
/// Existing files are never replaced, including by report export.
pub const NO_REPLACEMENT: &str = "Existing files are never replaced.";
/// Related resources verified absent from an existing matching copy.
pub const EXISTING_RELATED_RESOURCE_NOTICE: &str = "Inline images or other related resources are absent from the matching bytes. They may appear missing in the existing copy.";
fn synced(file: Option<crate::SyncReceipt>, namespace: crate::NamespaceObservation) -> bool {
    file.is_some_and(crate::SyncReceipt::succeeded)
        && namespace.required_edges > 0
        && namespace.required_edges <= 513
        && namespace.completed_edges == namespace.required_edges
        && namespace.failed_edge_index.is_none()
        && namespace.receipt.is_some_and(crate::SyncReceipt::succeeded)
}
/// Classify accepted display facts without granting any filesystem capability.
pub fn classify(f: &OutcomeFacts) -> OutcomeMeaning {
    use crate::{AddressState, CleanupState, PlanAction};
    let cleanup_ok = matches!(
        f.cleanup.state,
        CleanupState::NotNeeded | CleanupState::Complete
    ) && f.cleanup.native_code.is_none()
        && f.cleanup.residual_name.is_none();
    let existing_matches = f
        .existing
        .as_ref()
        .is_some_and(|e| e.bytes_equal == Some(true));
    let new_qualified = f.publication == Publication::Published
        && f.candidate_identity_known
        && f.address == AddressState::Verified
        && f.final_readback == Some(true)
        && synced(f.file_sync, f.namespace_sync)
        && cleanup_ok;
    let existing_qualified = f.existing.as_ref().is_some_and(|e| {
        e.identity_known
            && e.bytes_equal == Some(true)
            && e.stable
            && e.address == AddressState::Verified
            && synced(e.file_sync, e.namespace_sync)
    });
    let kind = match f.result {
        ResultKind::Planned
            if f.publication == Publication::NotPublished
                && f.error.is_none()
                && cleanup_ok
                && f.file_sync.is_none()
                && f.namespace_sync.receipt.is_none()
                && f.namespace_sync.required_edges == 0 =>
        {
            match f.plan {
                Some(PlanAction::Create) if f.existing.is_none() => MeaningKind::Prepared,
                Some(PlanAction::VerifyExisting) if existing_matches => MeaningKind::Comparison,
                _ => MeaningKind::Unestablished,
            }
        }
        ResultKind::Created
            if f.plan.is_none() && f.error.is_none() && new_qualified && f.existing.is_none() =>
        {
            MeaningKind::Created
        }
        ResultKind::Existing
            if f.plan.is_none()
                && f.publication == Publication::NotPublished
                && f.error.is_none()
                && existing_qualified
                && cleanup_ok =>
        {
            MeaningKind::ExistingVerified
        }
        ResultKind::Failed if f.plan.is_none() => match f.publication {
            Publication::Published => MeaningKind::PublishedIncomplete,
            Publication::Unknown => MeaningKind::PublicationUncertain,
            Publication::NotPublished if existing_matches => MeaningKind::ExistingMatched,
            Publication::NotPublished => MeaningKind::NotPublished,
        },
        _ => MeaningKind::Unestablished,
    };
    let removal_mode = match kind {
        MeaningKind::Created | MeaningKind::PublishedIncomplete => RemovalMode::Removed,
        MeaningKind::ExistingVerified => RemovalMode::Absent,
        _ => RemovalMode::Selected,
    };
    let mut consequences = Vec::new();
    match kind {
        MeaningKind::PublishedIncomplete => consequences.push(Consequence::Published),
        MeaningKind::PublicationUncertain => consequences.push(Consequence::PublicationUnknown),
        MeaningKind::ExistingMatched => {
            consequences.extend([Consequence::NoPublication, Consequence::ExistingUnqualified])
        }
        MeaningKind::NotPublished => consequences.push(Consequence::NoPublication),
        MeaningKind::Unestablished => match f.publication {
            Publication::Published => consequences.push(Consequence::Published),
            Publication::Unknown => consequences.push(Consequence::PublicationUnknown),
            Publication::NotPublished => {}
        },
        _ => {}
    }
    if matches!(
        kind,
        MeaningKind::PublishedIncomplete | MeaningKind::PublicationUncertain
    ) {
        if f.address != AddressState::Verified {
            consequences.push(Consequence::AddressUnqualified);
        }
        if !synced(f.file_sync, f.namespace_sync) {
            consequences.push(Consequence::DurabilityUnqualified);
        }
    }
    match f.cleanup.state {
        CleanupState::Residual => consequences.push(Consequence::ResidualCleanup),
        CleanupState::Unknown => consequences.push(Consequence::UnknownCleanup),
        _ if !cleanup_ok => consequences.push(Consequence::UnknownCleanup),
        _ => {}
    }
    OutcomeMeaning {
        kind,
        severity: match kind {
            MeaningKind::Created | MeaningKind::ExistingVerified => Severity::Success,
            MeaningKind::Prepared | MeaningKind::Comparison => Severity::Information,
            _ => Severity::Attention,
        },
        removal_mode,
        consequences,
        related_notice: f.related_resources.then_some(match removal_mode {
            RemovalMode::Removed => RELATED_RESOURCE_NOTICE,
            RemovalMode::Absent => EXISTING_RELATED_RESOURCE_NOTICE,
            RemovalMode::Selected => PREPARED_RELATED_RESOURCE_NOTICE,
        }),
    }
}

impl From<&crate::ExistingObservation> for ExistingFacts {
    fn from(value: &crate::ExistingObservation) -> Self {
        Self {
            identity_known: value.identity.is_some(),
            bytes_equal: value.bytes_equal,
            stable: value.stable,
            address: value.address,
            file_sync: value.file_sync,
            namespace_sync: value.namespace_sync,
        }
    }
}
impl From<&crate::ItemResult<'_>> for OutcomeFacts {
    fn from(result: &crate::ItemResult<'_>) -> Self {
        use crate::{CleanupReceipt, CleanupState, FailureEffect, ItemResult};
        let mut facts = Self {
            result: ResultKind::Failed,
            plan: None,
            publication: Publication::NotPublished,
            address: crate::AddressState::NotChecked,
            candidate_identity_known: false,
            final_readback: None,
            file_sync: None,
            namespace_sync: crate::NamespaceObservation::unattempted(),
            cleanup: CleanupReceipt {
                state: CleanupState::NotNeeded,
                residual_name: None,
                native_code: None,
            },
            existing: None,
            error: None,
            related_resources: false,
        };
        let metadata = match result {
            ItemResult::Created(copy, data) => {
                facts.result = ResultKind::Created;
                facts.set_published(copy.publication());
                // This private success value follows the publisher's exact
                // readback; a mere failed publication never supplies this fact.
                facts.final_readback = Some(true);
                Some(*data)
            }
            ItemResult::UpToDate(copy, data) => {
                facts.result = ResultKind::Existing;
                facts.existing = Some(copy.observation().into());
                Some(*data)
            }
            ItemResult::Planned(plan) => {
                facts.result = ResultKind::Planned;
                facts.plan = Some(plan.action());
                facts.existing = plan.existing().map(Into::into);
                Some(plan.metadata())
            }
            ItemResult::Failed(failure) => {
                facts.error = Some(failure.error().code());
                facts.existing = failure.existing().map(Into::into);
                match failure.effect() {
                    FailureEffect::Published(copy) => facts.set_published(copy),
                    FailureEffect::NotPublished {
                        candidate_identity,
                        address,
                        file_sync,
                        namespace_sync,
                        cleanup,
                    } => {
                        facts.candidate_identity_known = candidate_identity.is_some();
                        facts.address = *address;
                        facts.file_sync = *file_sync;
                        facts.namespace_sync = *namespace_sync;
                        facts.cleanup = cleanup.clone();
                    }
                    FailureEffect::Unknown(copy) => {
                        facts.publication = Publication::Unknown;
                        facts.candidate_identity_known = copy.candidate_identity().is_some();
                        facts.address = copy.address();
                        facts.file_sync = copy.file_sync();
                        facts.namespace_sync = copy.namespace_sync();
                        facts.cleanup = copy.cleanup().clone();
                    }
                }
                failure.metadata()
            }
        };
        facts.related_resources = metadata.is_some_and(|data| {
            data.removed
                .iter()
                .any(|part| part.reason == crate::RemovalReason::RelatedResource)
        });
        facts
    }
}
impl OutcomeFacts {
    fn set_published(&mut self, copy: &crate::PublishedCopy) {
        self.publication = Publication::Published;
        self.candidate_identity_known = true;
        self.address = copy.address();
        self.file_sync = copy.file_sync();
        self.namespace_sync = copy.namespace_sync();
        self.cleanup = copy.cleanup().clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AddressState, CleanupReceipt, CleanupState, NamespaceObservation, PlanAction, SyncMethod,
        SyncReceipt, SyncState,
    };
    fn facts() -> OutcomeFacts {
        let synced = SyncReceipt {
            method: SyncMethod::Fsync,
            state: SyncState::Succeeded,
            native_code: None,
        };
        OutcomeFacts {
            result: ResultKind::Created,
            plan: None,
            publication: Publication::Published,
            address: AddressState::Verified,
            candidate_identity_known: true,
            final_readback: Some(true),
            file_sync: Some(synced),
            namespace_sync: NamespaceObservation {
                receipt: Some(synced),
                required_edges: 1,
                completed_edges: 1,
                failed_edge_index: None,
            },
            cleanup: CleanupReceipt {
                state: CleanupState::Complete,
                residual_name: None,
                native_code: None,
            },
            existing: None,
            error: None,
            related_resources: true,
        }
    }
    #[test]
    fn missing_or_contradictory_success_facts_never_become_success() {
        assert_eq!(classify(&facts()).kind, MeaningKind::Created);
        for index in 0..8 {
            let mut f = facts();
            match index {
                0 => f.final_readback = None,
                1 => f.candidate_identity_known = false,
                2 => f.address = AddressState::Unknown,
                3 => f.file_sync = None,
                4 => f.namespace_sync.completed_edges = 0,
                5 => f.cleanup.native_code = Some(5),
                6 => f.error = Some(crate::ErrorCode::FileSyncFailed),
                _ => f.plan = Some(PlanAction::Create),
            }
            let m = classify(&f);
            assert_eq!(m.kind, MeaningKind::Unestablished);
            assert!(m.consequences.contains(&Consequence::Published));
        }
    }
    #[test]
    fn contradictory_native_sync_facts_cannot_render_created_or_existing_success() {
        for existing in [false, true] {
            for namespace in [false, true] {
                for code in [Some(0), Some(5)] {
                    let mut f = facts();
                    let receipt = SyncReceipt {
                        native_code: code,
                        ..f.file_sync.unwrap()
                    };
                    if namespace {
                        f.namespace_sync.receipt = Some(receipt);
                    } else {
                        f.file_sync = Some(receipt);
                    }
                    if existing {
                        f.result = ResultKind::Existing;
                        f.publication = Publication::NotPublished;
                        f.existing = Some(ExistingFacts {
                            identity_known: true,
                            bytes_equal: Some(true),
                            stable: true,
                            address: AddressState::Verified,
                            file_sync: f.file_sync.take(),
                            namespace_sync: f.namespace_sync,
                        });
                        f.namespace_sync = NamespaceObservation::unattempted();
                    }
                    assert_eq!(classify(&f).kind, MeaningKind::Unestablished);
                }
            }
        }
        for code in [22, 45, 102] {
            let mut f = facts();
            let limited = SyncReceipt {
                method: SyncMethod::FsyncFullfsyncUnsupported,
                state: SyncState::Succeeded,
                native_code: Some(code),
            };
            f.file_sync = Some(limited);
            f.namespace_sync.receipt = Some(limited);
            assert_eq!(classify(&f).kind, MeaningKind::Created);
        }
    }
    #[test]
    fn primary_sync_failure_preserves_publication_and_residual_cleanup() {
        let mut f = facts();
        f.result = ResultKind::Failed;
        f.error = Some(crate::ErrorCode::NamespaceSyncFailed);
        f.namespace_sync.completed_edges = 0;
        f.cleanup.state = CleanupState::Residual;
        f.cleanup.residual_name = Some("owned-stage".into());
        f.cleanup.native_code = Some(5);
        let m = classify(&f);
        assert_eq!(m.kind, MeaningKind::PublishedIncomplete);
        assert_eq!(m.removal_mode, RemovalMode::Removed);
        assert_eq!(
            m.consequences,
            [
                Consequence::Published,
                Consequence::DurabilityUnqualified,
                Consequence::ResidualCleanup
            ]
        );
        f.publication = Publication::NotPublished;
        assert_eq!(
            classify(&f).consequences,
            [Consequence::NoPublication, Consequence::ResidualCleanup]
        );
        f.publication = Publication::Unknown;
        f.cleanup.state = CleanupState::Unknown;
        assert_eq!(
            classify(&f).consequences,
            [
                Consequence::PublicationUnknown,
                Consequence::DurabilityUnqualified,
                Consequence::UnknownCleanup
            ]
        );
    }
    #[test]
    fn prepared_existing_and_verified_absence_have_distinct_meanings() {
        let mut f = facts();
        f.result = ResultKind::Planned;
        f.plan = Some(PlanAction::Create);
        f.publication = Publication::NotPublished;
        f.file_sync = None;
        f.namespace_sync = NamespaceObservation::unattempted();
        assert_eq!(classify(&f).kind, MeaningKind::Prepared);
        assert_eq!(classify(&f).removal_mode, RemovalMode::Selected);
        let qualified = facts();
        f.existing = Some(ExistingFacts {
            identity_known: true,
            bytes_equal: Some(true),
            stable: true,
            address: AddressState::Verified,
            file_sync: qualified.file_sync,
            namespace_sync: qualified.namespace_sync,
        });
        f.plan = Some(PlanAction::VerifyExisting);
        assert_eq!(classify(&f).kind, MeaningKind::Comparison);
        f.result = ResultKind::Existing;
        f.plan = None;
        assert_eq!(classify(&f).kind, MeaningKind::ExistingVerified);
        assert_eq!(classify(&f).removal_mode, RemovalMode::Absent);
        assert_eq!(
            classify(&f).related_notice,
            Some(EXISTING_RELATED_RESOURCE_NOTICE)
        );
        f.result = ResultKind::Failed;
        f.existing.as_mut().unwrap().file_sync = None;
        assert_eq!(classify(&f).kind, MeaningKind::ExistingMatched);
        assert_eq!(classify(&f).removal_mode, RemovalMode::Selected);
    }
}

/// Labels shared by run counts and result summaries.
#[derive(Clone, Copy, Debug)]
pub enum CountKind {
    /// Collected source positions known to the producer.
    KnownItems,
    /// Selected file or folder operands, before discovery.
    Selections,
    /// Original source storage length.
    SourceBytes,
    /// Verified candidate storage length.
    CopyBytes,
    /// MIME entity bytes described by removal metadata.
    PartBytes,

    /// New copies published successfully.
    Created,
    /// Existing matching copies verified.
    UpToDate,
    /// Read-only plans.
    Planned,
    /// Items that failed.
    Failed,
    /// Items that were never started.
    NotProcessed,
}
impl CountKind {
    /// User-facing count label.
    pub const fn label(self) -> &'static str {
        match self {
            Self::KnownItems => "known items",
            Self::Selections => "Selections",
            Self::SourceBytes => SOURCE_BYTES_LABEL,
            Self::CopyBytes => COPY_BYTES_LABEL,
            Self::PartBytes => "Bytes",

            Self::Created => "Created",
            Self::UpToDate => "up to date",
            Self::Planned => "planned",
            Self::Failed => "failed",
            Self::NotProcessed => "not processed",
        }
    }
}
/// Complete human summary using the same labels consumed by GUI count models.
pub fn summary(counts: &crate::Counts, exit: crate::ExitClass, interrupted: bool) -> String {
    format!(
        "{} {}; {} {}; {} {}; {} {}; {} {}. Exit {}{}",
        CountKind::Created.label(),
        counts.created(),
        CountKind::UpToDate.label(),
        counts.up_to_date(),
        CountKind::Planned.label(),
        counts.planned(),
        CountKind::Failed.label(),
        counts.failed(),
        CountKind::NotProcessed.label(),
        counts.not_processed(),
        exit.code(),
        if interrupted { "; interrupted" } else { "" }
    )
}
/// User-facing removal category; the MIME crate owns the decision itself.
pub const fn removal_reason(reason: crate::RemovalReason) -> &'static str {
    match reason {
        crate::RemovalReason::Attachment => "Attachment",
        crate::RemovalReason::RelatedResource => "Related image or resource",
        crate::RemovalReason::NonBodyPart => "Other non-body content",
    }
}

/// Desktop lifecycle labels shared with CLI outcome wording.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunStatus {
    /// Waiting selection.
    Queued,
    /// Read-only work is in flight.
    Checking,
    /// Copy creation is in flight.
    Creating,
    /// Successful terminal run.
    Completed,
    /// Terminal run containing failures.
    CompletedWithFailures,
    /// Cooperative interruption settled.
    Stopped,
    /// Interruption before a start record.
    StoppedBeforeStart,
    /// Forced termination with incomplete transport.
    ForceStopped,
    /// CLI admission refused the request.
    RequestRefused,
    /// Missing or invalid terminal observations.
    Incomplete,
    /// Bundled CLI identity differed.
    IdentityFailure,
    /// CLI could not be launched.
    StartFailed,
    /// A cooperative interrupt was requested.
    StopRequested,
    /// A forced termination was requested.
    ForceStopRequested,
}
impl RunStatus {
    /// Canonical user-facing lifecycle name.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Queued => "Not started",
            Self::Checking => "Checking",
            Self::Creating => "Creating copies",
            Self::Completed => "Completed",
            Self::CompletedWithFailures => "Completed with failures",
            Self::Stopped => "Stopped",
            Self::StoppedBeforeStart => "Stopped before start",
            Self::ForceStopped => "Force stopped; report incomplete",
            Self::RequestRefused => "Request refused",
            Self::Incomplete => "Incomplete report",
            Self::IdentityFailure => "Worker identity mismatch",
            Self::StartFailed => "Could not start the bundled CLI",
            Self::StopRequested => "Stop requested",
            Self::ForceStopRequested => "Force Stop requested",
        }
    }
}
/// The public name of a processing mode.
pub const fn mode_label(mode: crate::RunMode) -> &'static str {
    match mode {
        crate::RunMode::Create => "Create copies",
        crate::RunMode::DryRun => "Check only",
    }
}
/// Accessible count text, derived from the same count label as visible output.
pub fn count_label(kind: CountKind, value: u32) -> String {
    format!("{}: {value}", kind.label())
}
/// Selection snapshots are unaffected by later preference changes.
pub const DESTINATION_SCOPE: &str = "Destination changes apply to new requests.";
/// Incomplete reports retain accepted observations, without asserting what was not reported.
pub const INCOMPLETE_REPORT_NOTICE: &str = "Earlier results are retained. Work without a reported result is unknown; inspect the destination before retrying.";
/// A force-stopped process can leave recognizable staging residue.
pub const FORCE_STOP_NOTICE: &str = "A staging file named .emlattachmentremover-<random>.tmp may remain. It is never deleted automatically.";
/// Plain metadata count heading.
pub const SOURCE_BYTES_LABEL: &str = "Source bytes";
/// Prepared-byte count heading.
pub const COPY_BYTES_LABEL: &str = "Prepared copy bytes";

/// Successful report export.
pub const REPORT_SAVED: &str = "Report saved";
/// Failed report export without a completion observation.
pub const REPORT_EXPORT_FAILED: &str = "Report export did not complete";
/// Recovery from a bundled CLI identity mismatch.
pub const WORKER_IDENTITY_HINT: &str =
    "Install the app and its bundled CLI from the same download.";
/// Explain a paused serial queue using its actual report capacity.
pub fn report_space_notice(capacity: usize) -> String {
    format!("{capacity} reports are retained. Dismiss a report to start the waiting selection.")
}
