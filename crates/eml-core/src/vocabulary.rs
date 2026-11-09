//! Human-facing explanations shared by the CLI and desktop, outside wire facts.

/// Batch promise shared by human interfaces.
pub const ORIGINALS_UNCHANGED: &str = "Your originals are not changed.";

/// Material consequence of the body-only policy; HTML references are not rewritten.
pub const RELATED_RESOURCE_NOTICE: &str = "Inline images and other linked content were removed too. They may show as missing in the copy.";

/// Consequence for a prepared candidate that has not been confirmed as published.
pub const PREPARED_RELATED_RESOURCE_NOTICE: &str = "Inline images and other linked content will be removed too. They may show as missing in the copy.";
/// A partial inventory does not establish an I/O failure or a complete email count.
pub const COLLECTION_INCOMPLETE_NOTICE: &str = "Selection checking did not complete";

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
/// Observed facts for interpretation and report validation; construction grants no I/O authority.
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
    /// The candidate omits bytes even when no removal entry is reported.
    pub content_changed: bool,
}
/// A deliberate per-item stop, independent of run-wide interruption and side effects.
pub fn explicitly_interrupted(facts: &OutcomeFacts) -> bool {
    facts.result == ResultKind::Failed && facts.error == Some(crate::ErrorCode::Interrupted)
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
    /// No action needed.
    Neutral,
    /// Work awaits execution.
    Pending,
    /// A required operation failed.
    Failure,
    /// Qualification or certainty is incomplete.
    Attention,
}
impl Severity {
    /// Whether this prominence belongs in the shared problem count/filter.
    pub const fn is_problem(self) -> bool {
        matches!(self, Self::Attention | Self::Failure)
    }
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
    /// Cleanup completed while retaining a native failure observation.
    CleanupCompletedWithError,
}
impl Consequence {
    /// Canonical explanation and next action, without interpolated paths.
    pub const fn text(self) -> &'static str {
        match self {
            Self::Published => {
                "The copy was written, but saving it to disk couldn't be confirmed. Check the copy before running again."
            }
            Self::PublicationUnknown => {
                "A copy may have been written. Check the destination before running again."
            }
            Self::NoPublication => "No new copy was written.",
            Self::ExistingUnqualified => {
                "A matching copy already exists, but its save to disk couldn't be confirmed."
            }
            Self::AddressUnqualified => "The destination folder couldn't be confirmed.",
            Self::DurabilityUnqualified => "Saving to disk couldn't be confirmed.",
            Self::ResidualCleanup => {
                "A temporary file was left behind. Its name is in the details."
            }
            Self::UnknownCleanup => {
                "Removing a temporary file couldn't be confirmed. See the details."
            }
            Self::CleanupCompletedWithError => {
                "A temporary file was removed, but the system reported an error. See the details."
            }
        }
    }
}
/// Single shared interpretation; raw facts remain available in technical reports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutcomeMeaning {
    /// Conservative semantic category.
    pub kind: MeaningKind,
    /// Removal/stale-field heading mode.
    pub removal_mode: RemovalMode,
    /// At most eight simultaneous consequences, in stable display order.
    pub consequences: Vec<Consequence>,
    /// Related-resource notice appropriate to this exact qualification.
    pub related_notice: Option<&'static str>,
}
/// Related resources verified absent from an existing matching copy.
pub const EXISTING_RELATED_RESOURCE_NOTICE: &str = "The existing copy has no inline images or other linked content. They may show as missing in it.";
/// Classify accepted display facts without granting any filesystem capability.
pub fn classify(f: &OutcomeFacts) -> OutcomeMeaning {
    use crate::{AddressState, CleanupState, PlanAction};
    let cleanup_ok = f.cleanup.succeeded();
    let existing_matches = f
        .existing
        .as_ref()
        .is_some_and(|e| e.bytes_equal == Some(true));
    let new_qualified = crate::qualification::publication(
        f.publication,
        f.candidate_identity_known,
        f.address,
        f.file_sync,
        f.namespace_sync,
        &f.cleanup,
    ) && f.final_readback == Some(true);
    let existing_qualified = f.existing.as_ref().is_some_and(|e| {
        crate::qualification::existing(
            e.identity_known,
            e.bytes_equal,
            e.stable,
            e.address,
            e.file_sync,
            e.namespace_sync,
        )
    });
    let kind = match f.result {
        ResultKind::Planned
            if f.publication == Publication::NotPublished
                && f.error.is_none()
                && cleanup_ok
                && f.file_sync.is_none()
                && f.namespace_sync.is_unattempted() =>
        {
            match f.plan {
                Some(PlanAction::Create) if f.existing.is_none() => MeaningKind::Prepared,
                Some(PlanAction::VerifyExisting)
                    if f.existing.as_ref().is_some_and(|e| {
                        e.identity_known
                            && e.bytes_equal == Some(true)
                            && e.stable
                            && e.file_sync.is_none()
                            && e.namespace_sync.is_unattempted()
                    }) =>
                {
                    MeaningKind::Comparison
                }
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
        if !crate::qualification::synchronized(f.file_sync, f.namespace_sync) {
            consequences.push(Consequence::DurabilityUnqualified);
        }
    }
    match f.cleanup.state {
        CleanupState::Residual => consequences.push(Consequence::ResidualCleanup),
        CleanupState::Unknown => consequences.push(Consequence::UnknownCleanup),
        CleanupState::Complete
            if f.cleanup.native_code.is_some() && f.cleanup.residual_name.is_none() =>
        {
            consequences.push(Consequence::CleanupCompletedWithError);
        }
        _ if !cleanup_ok => consequences.push(Consequence::UnknownCleanup),
        _ => {}
    }
    OutcomeMeaning {
        kind,
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
            content_changed: false,
        };
        match result {
            ItemResult::Created(copy, _) => {
                facts.result = ResultKind::Created;
                facts.set_published(copy.publication());
                // This private success value follows the publisher's exact
                // readback; a mere failed publication never supplies this fact.
                facts.final_readback = Some(true);
            }
            ItemResult::UpToDate(copy, _) => {
                facts.result = ResultKind::Existing;
                facts.existing = Some(copy.observation().into());
            }
            ItemResult::Planned(plan) => {
                facts.result = ResultKind::Planned;
                facts.plan = Some(plan.action());
                facts.existing = plan.existing().map(Into::into);
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
            }
        };
        facts.content_changed = result
            .metadata()
            .is_some_and(|m| m.source_bytes != m.copy_bytes);
        facts.related_resources = result.metadata().is_some_and(|data| {
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
            content_changed: false,
        }
    }
    #[test]
    fn explicit_stop_requires_both_failed_result_and_typed_interrupted_error() {
        let mut f = facts();
        for result in [
            ResultKind::Created,
            ResultKind::Existing,
            ResultKind::Planned,
            ResultKind::Failed,
        ] {
            for error in [
                None,
                Some(crate::ErrorCode::Interrupted),
                Some(crate::ErrorCode::Malformed),
                Some(crate::ErrorCode::FileSyncFailed),
            ] {
                f.result = result;
                f.error = error;
                assert_eq!(
                    explicitly_interrupted(&f),
                    result == ResultKind::Failed && error == Some(crate::ErrorCode::Interrupted)
                );
            }
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
        assert_eq!(classify(&f).consequences, [Consequence::ResidualCleanup]);
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
            file_sync: None,
            namespace_sync: NamespaceObservation::unattempted(),
        });
        f.plan = Some(PlanAction::VerifyExisting);
        assert_eq!(classify(&f).kind, MeaningKind::Comparison);
        f.result = ResultKind::Existing;
        f.plan = None;
        f.existing.as_mut().unwrap().file_sync = qualified.file_sync;
        f.existing.as_mut().unwrap().namespace_sync = qualified.namespace_sync;
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

/// User-level result, independent of presentation and transport.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Outcome {
    /// A checked copy has attachments to remove.
    Ready,
    /// A checked message has no attachments.
    NothingToRemove,
    /// A qualified copy was saved with removals.
    CopySaved,
    /// A qualified copy was saved unchanged.
    CopiedAsIs,
    /// Matching existing bytes were qualified.
    AlreadyExists,
    /// An existing candidate awaits qualification.
    WillVerify,
    /// Saved or matching bytes need qualification.
    NeedsChecking,
    /// The result cannot be established.
    Unknown,
    /// Explicitly interrupted work.
    NotProcessed,
    /// Failure before publication, with its semantic diagnostic.
    Failed(crate::ErrorCode),
}
impl Outcome {
    /// Interpret independently qualified facts and the accepted removal count.
    pub fn from(facts: &OutcomeFacts, removed: usize) -> Self {
        if explicitly_interrupted(facts) {
            return Self::NotProcessed;
        }
        match classify(facts).kind {
            MeaningKind::Prepared if removed > 0 || facts.content_changed => Self::Ready,
            MeaningKind::Prepared => Self::NothingToRemove,
            MeaningKind::Comparison => Self::WillVerify,
            MeaningKind::Created if removed > 0 || facts.content_changed => Self::CopySaved,
            MeaningKind::Created => Self::CopiedAsIs,
            MeaningKind::ExistingVerified => Self::AlreadyExists,
            MeaningKind::PublishedIncomplete | MeaningKind::ExistingMatched => Self::NeedsChecking,
            MeaningKind::PublicationUncertain | MeaningKind::Unestablished => Self::Unknown,
            MeaningKind::NotPublished => facts.error.map_or(Self::Unknown, Self::Failed),
        }
    }
    /// Short wording shared by every human renderer.
    pub fn label(self) -> &'static str {
        match self {
            Self::Ready => "Ready",
            Self::NothingToRemove => "No attachments",
            Self::CopySaved => "Copy saved",
            Self::CopiedAsIs => "Copied as is",
            Self::AlreadyExists => "Copy already exists",
            Self::WillVerify => "Copy exists, will verify",
            Self::NeedsChecking => "Saved, needs checking",
            Self::Unknown => "Result unknown",
            Self::NotProcessed => "Not processed",
            Self::Failed(code) => failure(code).0,
        }
    }
    /// Count wording shared by summaries and the desktop header.
    pub fn count_label(self, n: usize) -> String {
        let phrase = match self {
            Self::Ready => "ready",
            Self::NothingToRemove => "with no attachments",
            Self::CopySaved if n == 1 => "copy saved",
            Self::CopySaved => "copies saved",
            Self::CopiedAsIs => "copied as is",
            Self::AlreadyExists => "already existed",
            Self::WillVerify if n == 1 => "copy to verify",
            Self::WillVerify => "copies to verify",
            Self::NeedsChecking => "saved, need checking",
            Self::Unknown => "results unknown",
            Self::NotProcessed => "not processed",
            Self::Failed(_) => "can’t be copied",
        };
        format!("{} {phrase}", format_count(n))
    }
    /// Domain prominence; frontends choose its appearance.
    pub fn severity(self) -> Severity {
        match self {
            Self::Ready | Self::WillVerify => Severity::Pending,
            Self::NothingToRemove | Self::NotProcessed => Severity::Neutral,
            Self::CopySaved | Self::CopiedAsIs | Self::AlreadyExists => Severity::Success,
            Self::NeedsChecking | Self::Unknown => Severity::Attention,
            Self::Failed(_) => Severity::Failure,
        }
    }
    /// Plain explanation of the observed result.
    pub fn explanation(self) -> &'static str {
        match self {
            Self::Ready => "These will be removed from the copy:",
            Self::NothingToRemove | Self::CopiedAsIs => {
                "Nothing to remove. The copy keeps the whole message."
            }
            Self::WillVerify => {
                "A copy with this name already exists. It will be checked, never replaced."
            }
            Self::CopySaved => "Copy saved. Removed:",
            Self::AlreadyExists => "An identical copy already exists. Nothing was written.",
            Self::NotProcessed => {
                "Not copied because the batch was stopped. Try again to finish it."
            }
            Self::Unknown => {
                "The result couldn’t be confirmed. Check the save folder, then try again. Existing copies are never replaced."
            }
            Self::NeedsChecking => {
                "A copy exists, but the final checks didn’t complete. Check the save folder, then try again. Existing copies are never replaced."
            }
            Self::Failed(code) => failure(code).1,
        }
    }
}
fn failure(code: crate::ErrorCode) -> (&'static str, &'static str) {
    use crate::ErrorCode::*;
    match code {
        MailboxFile => (
            "Several emails in one file",
            "This file holds several emails (a mailbox export). Export the emails one at a time, then add them.",
        ),
        Malformed => (
            "Not a valid email",
            "This file isn't a valid email. It may be empty or not an email at all; if it came from your mail app, export it again.",
        ),
        NoBody => (
            "No text to keep",
            "This email has no plain-text or HTML body, so a copy would be empty.",
        ),
        Protected => (
            "Protected content",
            "This email contains signed or encrypted content that can’t be changed safely.",
        ),
        UnsupportedMime => (
            "Unsupported format",
            "This email uses a structure the app doesn’t support.",
        ),
        LimitExceeded => (
            "Too large or complex",
            "This email is too deeply nested or has too many parts.",
        ),
        ReadFailed | BindFailed => (
            "Can’t be read",
            "The file couldn’t be read. Check that it still exists and that you have permission to open it.",
        ),
        SourceNotRegular => (
            "Not a file",
            "This isn’t a regular file. Add the email file itself.",
        ),
        SourceChanged => (
            "Changed while reading",
            "The file changed while it was being read. Try again when nothing else is using it.",
        ),
        DestinationExists => (
            "Name already taken",
            "A different file already has the copy’s name. Move it or choose another save folder.",
        ),
        DestinationCollision => (
            "Duplicate copy name",
            "Two emails would get the same copy name. Choose a save folder, or keep copies next to the originals.",
        ),
        SourceDestinationAlias => (
            "Would overwrite original",
            "The copy would replace an original, so it was skipped.",
        ),
        DestinationUnsupported => (
            "Folder not supported",
            "The save folder doesn’t support safe copying. Choose another folder.",
        ),
        StageFailed | WriteFailed => (
            "Couldn’t save copy",
            "The copy couldn’t be written. Check free space and permissions in the save folder.",
        ),
        Internal => (
            "Couldn’t copy",
            "An internal check failed, so no copy was made. Include the technical details in a bug report.",
        ),
        _ => (
            "Couldn’t copy",
            "No copy was made. Check the technical details for more information.",
        ),
    }
}
/// Format decimal storage units consistently.
pub fn format_bytes(bytes: u64) -> String {
    if bytes < 1000 {
        return format!("{bytes} B");
    }
    for (scale, unit) in [(1_000_000_000, "GB"), (1_000_000, "MB"), (1000, "kB")] {
        if bytes >= scale {
            return format!("{:.1} {unit}", bytes as f64 / scale as f64);
        }
    }
    unreachable!()
}

/// Shared plain-language whole-run failure.
#[derive(Clone, Copy, Debug)]
pub struct RunFailure(pub crate::ErrorCode);
impl RunFailure {
    /// Short whole-run label.
    pub fn label(self) -> &'static str {
        Outcome::Failed(self.0).label()
    }
    /// Actionable explanation for the whole request.
    pub fn explanation(self) -> &'static str {
        use crate::ErrorCode::*;
        match self.0 {
            NoInputs => "No .eml files were found in what you added. Add other files or folders.",
            InputLimitExceeded => {
                "The selection exceeds the supported limits. Add fewer files or split large folders."
            }
            OutputOverlap => {
                "The save folder is inside a folder you added. Choose a different save folder."
            }
            InputAdmissionFailed => {
                "Some items you added can’t be opened. Check that they still exist, then try again."
            }
            OutputFailed => {
                "The copy process stopped while reporting results. Keep the technical details and try again."
            }
            _ => Outcome::Failed(self.0).explanation(),
        }
    }
}
/// Human part kind; the MIME policy remains owned by message preparation.
pub fn part_kind(reason: crate::RemovalReason, content_type: &str) -> &'static str {
    match reason {
        crate::RemovalReason::RelatedResource
            if content_type
                .get(..6)
                .is_some_and(|s| s.eq_ignore_ascii_case("image/")) =>
        {
            "Inline image"
        }
        crate::RemovalReason::RelatedResource => "Inline resource",
        crate::RemovalReason::NonBodyPart if content_type.eq_ignore_ascii_case("text/calendar") => {
            "Calendar invite"
        }
        crate::RemovalReason::NonBodyPart => "Other content",
        crate::RemovalReason::AfterMessageEnd => "Content after the email’s end",
        crate::RemovalReason::Attachment => "",
    }
}
/// An unnamed part is named by its kind, never its raw MIME type.
pub fn part_name<'a>(
    filename: Option<&'a str>,
    reason: crate::RemovalReason,
    content_type: &str,
) -> &'a str {
    filename
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| {
            let kind = part_kind(reason, content_type);
            if kind.is_empty() { "Attachment" } else { kind }
        })
}
/// Aggregate of human outcomes; no report or UI dependency.
#[derive(Clone, Debug, Default)]
pub struct BatchTotals {
    /// Total emails represented, including unprocessed work.
    pub emails: usize,
    /// Counts by the single user outcome.
    pub counts: std::collections::BTreeMap<Outcome, usize>,
    /// Attachments selected for removal or removed in qualified copies.
    pub attachments: usize,
    /// Bytes selected for removal or removed in qualified copies.
    pub bytes_removed: u64,
    /// Results requiring attention or reporting failure.
    pub problems: usize,
}
impl BatchTotals {
    /// Aggregate accepted outcomes, metadata and remaining work without item events.
    /// Row-only projections pass zero; remaining work must not also appear as rows.
    pub fn from(
        items: impl IntoIterator<Item = (Option<Outcome>, usize, Option<(u64, u64)>)>,
        unprocessed: usize,
    ) -> Self {
        let mut totals = Self::default();
        for (outcome, parts, bytes) in items {
            totals.emails += 1;
            let Some(outcome) = outcome else {
                continue;
            };
            *totals.counts.entry(outcome).or_default() += 1;
            if outcome.severity().is_problem() {
                totals.problems += 1;
            }
            if matches!(outcome, Outcome::Ready | Outcome::CopySaved) {
                totals.attachments += parts;
                if let Some((source, copy)) = bytes {
                    totals.bytes_removed += source.saturating_sub(copy);
                }
            }
        }
        totals.emails += unprocessed;
        if unprocessed > 0 {
            *totals.counts.entry(Outcome::NotProcessed).or_default() += unprocessed;
        }
        totals
    }
    /// Count represented positions whose interface failed before work could start.
    /// Their positions were already included; do not manufacture core outcomes or emails.
    pub fn with_unclassified_failures(mut self, count: usize) -> Self {
        self.problems += count;
        self
    }
    /// Number of emails with this exact outcome.
    pub fn count(&self, outcome: Outcome) -> usize {
        self.counts.get(&outcome).copied().unwrap_or(0)
    }
    /// Number of newly saved copies, including unchanged messages.
    pub fn saved(&self) -> usize {
        self.count(Outcome::CopySaved) + self.count(Outcome::CopiedAsIs)
    }
    /// Whether any result has unconfirmed publication or qualification.
    pub fn has_attention(&self) -> bool {
        self.counts
            .keys()
            .any(|o| o.severity() == Severity::Attention)
    }
    /// Shared problem count wording, preserving attention versus failure.
    pub fn problem_label(&self) -> String {
        if self.has_attention() {
            format!(
                "{} {} attention",
                self.problems,
                if self.problems == 1 { "needs" } else { "need" }
            )
        } else {
            Outcome::Failed(crate::ErrorCode::Internal).count_label(self.problems)
        }
    }
    /// Removal consequence for check-only or completed copy metadata.
    pub fn removal_summary(&self, checking: bool) -> String {
        format!(
            "{} {}",
            format_bytes(self.bytes_removed),
            if checking {
                "would be removed"
            } else {
                "removed"
            }
        )
    }
    /// Include whole-run collection/admission facts without presenting a partial count.
    pub fn run_headline(
        &self,
        mode: HeadlineMode,
        error: Option<&crate::OperationFailure>,
    ) -> Headline {
        if let Some(verdict) = error.and_then(crate::OperationFailure::refusal_headline) {
            return Headline {
                verdict,
                parts: Vec::new(),
            };
        }
        self.headline(mode)
    }
    /// Shared verdict and nonzero supporting facts for a settled batch.
    pub fn headline(&self, mode: HeadlineMode) -> Headline {
        let mut parts = Vec::new();
        let verdict = match mode {
            HeadlineMode::Check { total } => {
                if self.attachments > 0 {
                    parts.push(attachment_count(self.attachments));
                    if self.bytes_removed > 0 {
                        parts.push(self.removal_summary(true));
                    }
                } else {
                    let n = self.count(Outcome::NothingToRemove);
                    if n > 0 {
                        parts.push(Outcome::NothingToRemove.count_label(n));
                    }
                }
                let n = self.count(Outcome::WillVerify);
                if n > 0 {
                    parts.push(Outcome::WillVerify.count_label(n));
                }
                total.map_or_else(|| "Emails not checked".into(), email_count)
            }
            HeadlineMode::Stopped => {
                for (outcome, n) in [
                    (Outcome::CopySaved, self.saved()),
                    (Outcome::NotProcessed, self.count(Outcome::NotProcessed)),
                ] {
                    if n > 0 {
                        parts.push(outcome.count_label(n));
                    }
                }
                "Stopped".into()
            }
            HeadlineMode::Done { incomplete_create } => {
                if self.bytes_removed > 0 {
                    parts.push(self.removal_summary(false));
                }
                for outcome in [Outcome::CopiedAsIs, Outcome::AlreadyExists] {
                    let n = self.count(outcome);
                    let counted_in_verdict = outcome == Outcome::AlreadyExists
                        && self.saved() == 0
                        && !self.has_attention()
                        && !incomplete_create;
                    if n > 0 && !counted_in_verdict {
                        parts.push(outcome.count_label(n));
                    }
                }
                let saved = self.saved();
                if saved == 0 && (self.has_attention() || incomplete_create) {
                    "Results need attention".into()
                } else if saved == 0 && self.count(Outcome::AlreadyExists) > 0 {
                    let n = self.count(Outcome::AlreadyExists);
                    format!(
                        "{} {} already {}",
                        format_count(n),
                        if n == 1 { "copy" } else { "copies" },
                        if n == 1 { "exists" } else { "exist" }
                    )
                } else {
                    if saved == 0 {
                        "No copies saved".into()
                    } else {
                        Outcome::CopySaved.count_label(saved)
                    }
                }
            }
        };
        if self.problems > 0 {
            parts.push(self.problem_label());
        }
        Headline { verdict, parts }
    }
}
/// Settled batch context, independent of presentation and transport.
#[derive(Clone, Copy, Debug)]
pub enum HeadlineMode {
    /// Read-only result with the admitted inventory, if established.
    Check {
        /// Number of discovered emails.
        total: Option<usize>,
    },
    /// A completed copy attempt, including incomplete worker observations.
    Done {
        /// Whether the copy worker ended without a complete result.
        incomplete_create: bool,
    },
    /// Deliberately stopped work, using settled totals.
    Stopped,
}
/// A batch verdict and its nonzero supporting facts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Headline {
    /// Primary result sentence fragment.
    pub verdict: String,
    /// Supporting facts in their shared order.
    pub parts: Vec<String>,
}
/// Copy Details field labels shared with human diagnostic renderers.
pub mod fields {
    /// Original path.
    pub const SOURCE: &str = "Source";
    /// Copy path.
    pub const DESTINATION: &str = "Destination";
    /// Outcome label.
    pub const STATUS: &str = "Status";
    /// Original bytes.
    pub const SOURCE_BYTES: &str = "Source bytes";
    /// Copy bytes.
    pub const COPY_BYTES: &str = "Copy bytes";
    /// Technical diagnostic code.
    pub const CODE: &str = "Code";
    /// Technical diagnostic phase.
    pub const PHASE: &str = "Phase";
    /// Technical diagnostic explanation.
    pub const DETAIL: &str = "Detail";
    /// Cleanup observation, independent of the primary failure.
    pub const CLEANUP: &str = "Cleanup";
    /// Uninterpreted numeric native cause attached to an observation.
    pub const NATIVE_CODE: &str = "Native code";
    /// Owned staging basename retained after cleanup.
    pub const RESIDUAL_RESOURCE: &str = "Residual resource";
}

/// Human cleanup state shared by technical diagnostic renderers.
pub fn cleanup_label(state: crate::CleanupState) -> &'static str {
    match state {
        crate::CleanupState::NotNeeded => "Not needed",
        crate::CleanupState::Complete => "Complete",
        crate::CleanupState::Residual => "Residual",
        crate::CleanupState::Unknown => "Unknown",
    }
}

/// Shared email-count noun.
pub fn email_count(n: usize) -> String {
    format!(
        "{} {}",
        format_count(n),
        if n == 1 { "email" } else { "emails" }
    )
}
/// Shared attachment-count noun.
pub fn attachment_count(n: usize) -> String {
    format!(
        "{} {}",
        format_count(n),
        if n == 1 { "attachment" } else { "attachments" }
    )
}

/// Whether settled preview observations offer a fresh copy attempt. This never
/// authorizes execution: current inputs and destinations must always be readmitted.
pub fn can_offer_copy(
    outcomes: impl IntoIterator<Item = Outcome>,
    error: Option<crate::ErrorCode>,
) -> bool {
    if error.is_some_and(|code| {
        matches!(
            code,
            crate::ErrorCode::NoInputs
                | crate::ErrorCode::BindFailed
                | crate::ErrorCode::InputLimitExceeded
                | crate::ErrorCode::OutputOverlap
        )
    }) {
        return false;
    }
    outcomes.into_iter().any(|outcome| match outcome {
        Outcome::Failed(_) => false,
        Outcome::Ready
        | Outcome::NothingToRemove
        | Outcome::CopySaved
        | Outcome::CopiedAsIs
        | Outcome::AlreadyExists
        | Outcome::WillVerify
        | Outcome::NeedsChecking
        | Outcome::Unknown
        | Outcome::NotProcessed => true,
    })
}
/// Whether settled results cover the discovered inventory. This does not prove
/// current filesystem absence. Delivery failures remain a separate boundary fact.
pub fn results_cover_inventory(
    complete: bool,
    interrupted: bool,
    unprocessed: usize,
    error: Option<crate::ErrorCode>,
) -> bool {
    complete
        && !interrupted
        && unprocessed == 0
        && error.is_none_or(|code| code == crate::ErrorCode::NoInputs)
}

/// Canonical save-folder recovery wording for both front ends.
pub fn output_folder_unavailable(path: &std::path::Path) -> String {
    format!(
        "The save folder isn't available: {}. Reconnect it or choose another folder.",
        crate::display::DisplayPath::new(path)
    )
}
/// Known native write causes; unknown causes retain generic core guidance.
pub fn write_cause(error: &std::io::Error) -> Option<&'static str> {
    let code = error.raw_os_error()?;
    #[cfg(unix)]
    {
        use rustix::io::Errno;
        if [Errno::NOSPC.raw_os_error(), Errno::DQUOT.raw_os_error()].contains(&code) {
            return Some("The save folder's disk is full.");
        }
        if code == Errno::ROFS.raw_os_error() {
            return Some("The save folder is on a read-only disk.");
        }
        if [Errno::ACCESS.raw_os_error(), Errno::PERM.raw_os_error()].contains(&code) {
            return Some("You don't have permission to save in this folder.");
        }
    }
    #[cfg(windows)]
    {
        if [39, 112, 1295].contains(&code) {
            return Some("The save folder's disk is full.");
        }
        if code == 19 {
            return Some("The save folder is on a read-only disk.");
        }
        if code == 5 {
            return Some("You don't have permission to save in this folder.");
        }
    }
    None
}
/// Single human count formatter; UI and CLI counts deliberately use no grouping.
pub fn format_count(value: usize) -> String {
    value.to_string()
}

/// Structured JSON-request reason; the protocol adapter supplies facts, not prose.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RequestDocumentReason {
    /// An unsupported key was named.
    UnknownField(String),
    /// A key appeared more than once.
    DuplicateField(String),
    /// Bytes followed the complete document.
    TrailingCharacters,
    /// The document ended before completion.
    EndOfDocument,
    /// Invalid JSON syntax or UTF-8.
    Syntax,
    /// The JSON value has an incompatible type/shape.
    Type,
}
/// One safe authored request-document diagnostic, with no raw JSON values.
pub fn request_document_error(
    line: usize,
    column: usize,
    reason: &RequestDocumentReason,
) -> String {
    use crate::display::DisplayText;
    let reason = match reason {
        RequestDocumentReason::UnknownField(name) if name.is_empty() => {
            "unknown field (name omitted)".into()
        }
        RequestDocumentReason::UnknownField(name) => {
            format!("unknown field `{}`", DisplayText::new(name))
        }
        RequestDocumentReason::DuplicateField(name) if name.is_empty() => {
            "duplicate field (name omitted)".into()
        }
        RequestDocumentReason::DuplicateField(name) => {
            format!("duplicate field `{}`", DisplayText::new(name))
        }
        RequestDocumentReason::TrailingCharacters => "trailing characters".into(),
        RequestDocumentReason::EndOfDocument => "unexpected end of document (EOF)".into(),
        RequestDocumentReason::Syntax => "invalid JSON syntax".into(),
        RequestDocumentReason::Type => "invalid document type".into(),
    };
    format!(
        "Invalid request document at line {}, column {}: {reason}",
        format_count(line),
        format_count(column)
    )
}
/// Interface I/O context whose human wording is shared and owned here.
#[derive(Clone, Copy)]
pub enum InterfaceIo {
    /// Read the CLI request document.
    ReadRequest,
    /// Start the CLI request reader.
    StartRequest,
    /// Capture the caller's working folder.
    WorkingFolder,
    /// Save the desktop preferences file.
    SaveSettings,
}
/// Render a native failure without localized Rust ErrorKind text.
pub fn interface_io_error(operation: InterfaceIo, error: &std::io::Error) -> String {
    let sentence = match operation {
        InterfaceIo::ReadRequest => {
            "The request document couldn't be read. Check its path and permissions."
        }
        InterfaceIo::StartRequest => "The request reader couldn't be started. Try again.",
        InterfaceIo::WorkingFolder => {
            "The working folder isn't available. Choose an existing folder and try again."
        }
        InterfaceIo::SaveSettings => {
            "The app couldn't save its settings. Check the settings folder's permissions."
        }
    };
    error.raw_os_error().map_or_else(
        || sentence.into(),
        |code| {
            format!(
                "{sentence}\n[Native cause: {}]",
                crate::NativeDiagnostic::OsCode(code)
            )
        },
    )
}
/// Request-reader capacity failure.
pub const REQUEST_DOCUMENT_TOO_LARGE: &str = "Request document exceeds 8 MiB";
/// Stop handling could not be established.
pub const STOP_HANDLER_UNAVAILABLE: &str = "Stop requests couldn't be enabled. Try again.";
/// The request reader disappeared without a complete document.
pub const REQUEST_READER_ENDED: &str =
    "The request reader stopped before the document was complete.";

/// Selection-limit refusal, without presenting the retained prefix as the selection.
pub fn too_many_emails() -> String {
    format!(
        "Too many emails (more than {})",
        format_count(crate::MAX_COLLECTED_ITEMS)
    )
}

impl RequestDocumentReason {
    /// Keep short field names useful without echoing large/control-bearing request data.
    pub fn unknown_field(name: &str) -> Self {
        Self::UnknownField(safe_document_field(name))
    }
    /// Duplicate fields follow the same bounded diagnostic rule.
    pub fn duplicate_field(name: &str) -> Self {
        Self::DuplicateField(safe_document_field(name))
    }
}
fn safe_document_field(name: &str) -> String {
    if name.len() > 48 {
        return String::new();
    }
    let text = crate::display::DisplayText::new(name).to_string();
    if text == name && text.len() <= 48 {
        text
    } else {
        String::new()
    }
}
