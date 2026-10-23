//! Typed workflow observations. Native success is never inferred from a path.
use crate::{OperationFailure, RemovedPart};
use eml_fs::{CleanupReceipt, Identity, SyncReceipt};
use std::{io, path::Path};

/// Requested-address observation, independent of object identity or publication.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddressState {
    /// Address qualification was not attempted.
    NotChecked,
    /// Required address checks completed against the held object.
    Verified,
    /// A comparison proved a different requested address.
    Changed,
    /// Address qualification could not be completed.
    Unknown,
}
/// Bounded aggregate of required ordered namespace operations, not an edge trace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NamespaceObservation {
    /// Aggregate method/outcome qualified across the counted edge set, retaining
    /// a failed edge's receipt. A last-edge success alone cannot establish this
    /// aggregate. None means no synchronization was attempted.
    pub receipt: Option<SyncReceipt>,
    /// Number of required ordered operations.
    pub required_edges: u16,
    /// Successfully completed operations, never more than required_edges.
    pub completed_edges: u16,
    /// Failed operation's zero-based index, if one was attempted and failed.
    pub failed_edge_index: Option<u16>,
}
impl NamespaceObservation {
    pub(super) const fn unattempted() -> Self {
        Self {
            receipt: None,
            required_edges: 0,
            completed_edges: 0,
            failed_edge_index: None,
        }
    }
}
/// Current existing-file knowledge; equality alone does not qualify UpToDate.
#[derive(Clone, Debug)]
pub struct ExistingObservation {
    /// Actual observed entry or opened-file identity, when established.
    pub identity: Option<Identity>,
    /// Complete comparison result, absent when comparison did not complete.
    pub bytes_equal: Option<bool>,
    /// Before/after existing-binding checks completed unchanged.
    pub stable: bool,
    /// Requested destination address qualification.
    pub address: AddressState,
    /// Actual file synchronization; None is explicitly unattempted.
    pub file_sync: Option<SyncReceipt>,
    /// Actual namespace synchronization.
    pub namespace_sync: NamespaceObservation,
}
/// Actual new publication evidence, permitted to contain failed qualification.
/// This is independent of CreatedCopy's complete successful qualification.
#[derive(Debug)]
pub struct PublishedCopy {
    identity: Identity,
    address: AddressState,
    file_sync: Option<SyncReceipt>,
    namespace_sync: NamespaceObservation,
    cleanup: CleanupReceipt,
}
impl PublishedCopy {
    pub(super) fn observed(
        identity: Identity,
        address: AddressState,
        file_sync: Option<SyncReceipt>,
        namespace_sync: NamespaceObservation,
        cleanup: CleanupReceipt,
    ) -> Self {
        Self {
            identity,
            address,
            file_sync,
            namespace_sync,
            cleanup,
        }
    }
    /// Identity retained through the actual publication edge.
    pub fn identity(&self) -> Identity {
        self.identity
    }
    /// Actual requested-address observation.
    pub fn address(&self) -> AddressState {
        self.address
    }
    /// Actual file synchronization, including any failure/unattempted state.
    pub fn file_sync(&self) -> Option<SyncReceipt> {
        self.file_sync
    }
    /// Actual namespace synchronization.
    pub fn namespace_sync(&self) -> NamespaceObservation {
        self.namespace_sync
    }
    /// Actual owned staging cleanup.
    pub fn cleanup(&self) -> &CleanupReceipt {
        &self.cleanup
    }
}
/// Successful creation qualification. Construction belongs to the real publisher;
/// a failure's PublishedCopy evidence cannot manufacture this value.
#[derive(Debug)]
pub struct CreatedCopy(PublishedCopy);
impl CreatedCopy {
    pub(super) fn qualify(publication: PublishedCopy) -> Result<Self, Box<PublishedCopy>> {
        if publication.address == AddressState::Verified
            && publication.file_sync.is_some_and(SyncReceipt::succeeded)
            && namespace_qualified(publication.namespace_sync)
            && matches!(
                publication.cleanup.state,
                eml_fs::CleanupState::NotNeeded | eml_fs::CleanupState::Complete
            )
            && publication.cleanup.native_code.is_none()
        {
            Ok(Self(publication))
        } else {
            Err(Box::new(publication))
        }
    }
    /// Successfully qualified publication evidence.
    pub fn publication(&self) -> &PublishedCopy {
        &self.0
    }
}
/// Successful existing-copy qualification, privately constructed after real sync
/// and address checks. Byte comparison alone cannot construct this value.
#[derive(Debug)]
pub struct ExistingCopy(ExistingObservation);
impl ExistingCopy {
    pub(super) fn qualify(observation: ExistingObservation) -> Result<Self, ExistingObservation> {
        if observation.identity.is_some()
            && observation.bytes_equal == Some(true)
            && observation.stable
            && observation.address == AddressState::Verified
            && observation.file_sync.is_some_and(SyncReceipt::succeeded)
            && namespace_qualified(observation.namespace_sync)
        {
            Ok(Self(observation))
        } else {
            Err(observation)
        }
    }
    /// Complete qualified existing observation.
    pub fn observation(&self) -> &ExistingObservation {
        &self.0
    }
}
/// Ambiguous publication evidence; no final verified identity is manufactured.
#[derive(Debug)]
pub struct UnknownPublication {
    candidate_identity: Option<Identity>,
    address: AddressState,
    file_sync: Option<SyncReceipt>,
    namespace_sync: NamespaceObservation,
    cleanup: CleanupReceipt,
}
impl UnknownPublication {
    pub(super) fn observed(
        candidate_identity: Option<Identity>,
        address: AddressState,
        file_sync: Option<SyncReceipt>,
        namespace_sync: NamespaceObservation,
        cleanup: CleanupReceipt,
    ) -> Self {
        Self {
            candidate_identity,
            address,
            file_sync,
            namespace_sync,
            cleanup,
        }
    }
    /// Known candidate identity, without a claim about the final entry.
    pub fn candidate_identity(&self) -> Option<Identity> {
        self.candidate_identity
    }
    /// Actual address knowledge.
    pub fn address(&self) -> AddressState {
        self.address
    }
    /// Actual candidate synchronization.
    pub fn file_sync(&self) -> Option<SyncReceipt> {
        self.file_sync
    }
    /// Actual namespace synchronization.
    pub fn namespace_sync(&self) -> NamespaceObservation {
        self.namespace_sync
    }
    /// Actual owned cleanup knowledge.
    pub fn cleanup(&self) -> &CleanupReceipt {
        &self.cleanup
    }
}
fn namespace_qualified(namespace: NamespaceObservation) -> bool {
    namespace.required_edges > 0
        && namespace.required_edges <= 513
        && namespace.completed_edges == namespace.required_edges
        && namespace.failed_edge_index.is_none()
        && namespace.receipt.is_some_and(SyncReceipt::succeeded)
}
/// Publication effect for a failed item, independent of its intended destination.
#[derive(Debug)]
pub enum FailureEffect {
    /// No edge published a new copy; retain actual pre-edge receipts.
    NotPublished {
        /// Candidate identity if one was established.
        candidate_identity: Option<Identity>,
        /// Actual pre-edge requested-address knowledge.
        address: AddressState,
        /// Actual attempted candidate file synchronization.
        file_sync: Option<SyncReceipt>,
        /// Actual attempted namespace synchronization.
        namespace_sync: NamespaceObservation,
        /// Actual owned staging cleanup.
        cleanup: CleanupReceipt,
    },
    /// The edge published, while a later requirement failed.
    Published(PublishedCopy),
    /// Publication could not be confirmed or disproved.
    Unknown(UnknownPublication),
}
/// Borrowed bounded verified metadata for the one currently processed message.
/// The event borrows no output byte buffer and RunOutcome retains no item list.
#[derive(Clone, Copy, Debug)]
pub struct MessageMetadata<'a> {
    /// Original held-source length.
    pub source_bytes: u64,
    /// Verified candidate length.
    pub copy_bytes: u64,
    /// Independently authorized source-order removals.
    pub removed: &'a [RemovedPart],
    /// Sorted unique stripped field names.
    pub stripped_fields: &'a [String],
}
/// Read-only action, never a synchronization promise.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlanAction {
    /// No existing ordinary entry was observed.
    Create,
    /// Existing stable bytes matched; normal execution still needs sync/address checks.
    VerifyExisting,
}
/// Successfully verified read-only plan, privately built by the serial workflow.
#[derive(Debug)]
pub struct PlannedCopy<'a> {
    pub(super) action: PlanAction,
    pub(super) metadata: MessageMetadata<'a>,
    pub(super) existing: Option<ExistingObservation>,
}
impl<'a> PlannedCopy<'a> {
    /// Planned action.
    pub fn action(&self) -> PlanAction {
        self.action
    }
    /// Verified bounded source metadata.
    pub fn metadata(&self) -> MessageMetadata<'a> {
        self.metadata
    }
    /// Existing-byte observation, without synchronization qualification.
    pub fn existing(&self) -> Option<&ExistingObservation> {
        self.existing.as_ref()
    }
}
/// Started terminal failure with independent effects, receipts and metadata.
#[derive(Debug)]
pub struct ItemFailure<'a> {
    pub(super) error: OperationFailure,
    pub(super) section: Option<String>,
    pub(super) metadata: Option<MessageMetadata<'a>>,
    pub(super) effect: FailureEffect,
    pub(super) existing: Option<ExistingObservation>,
}
impl<'a> ItemFailure<'a> {
    /// Bounded authored diagnostic; native cause is separately available.
    pub fn error(&self) -> &OperationFailure {
        &self.error
    }
    /// MIME section, if preparation supplied it.
    pub fn section(&self) -> Option<&str> {
        self.section.as_deref()
    }
    /// Metadata only when fully admitted against the retained-removal budget.
    pub fn metadata(&self) -> Option<MessageMetadata<'a>> {
        self.metadata
    }
    /// Actual publication effect and receipts.
    pub fn effect(&self) -> &FailureEffect {
        &self.effect
    }
    /// Existing observation independent of publication by this item.
    pub fn existing(&self) -> Option<&ExistingObservation> {
        self.existing.as_ref()
    }
}
/// Exactly one terminal item result. Successful qualification values have private
/// construction; a known destination never manufactures a publication result.
#[derive(Debug)]
pub enum ItemResult<'a> {
    /// Qualified new publication.
    Created(CreatedCopy, MessageMetadata<'a>),
    /// Qualified byte-identical resynchronized existing copy.
    UpToDate(ExistingCopy, MessageMetadata<'a>),
    /// Verified read-only plan.
    Planned(PlannedCopy<'a>),
    /// Actual failure, potentially with published/unknown evidence.
    Failed(Box<ItemFailure<'a>>),
}
/// Aggregate terminal accounting, computed once in the reusable workflow.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub(super) created: usize,
    pub(super) up_to_date: usize,
    pub(super) planned: usize,
    pub(super) failed: usize,
    pub(super) not_processed: usize,
}
impl Counts {
    /// Successfully qualified new copies.
    pub fn created(&self) -> usize {
        self.created
    }
    /// Successfully qualified existing copies.
    pub fn up_to_date(&self) -> usize {
        self.up_to_date
    }
    /// Completed read-only plans.
    pub fn planned(&self) -> usize {
        self.planned
    }
    /// Started terminal failures.
    pub fn failed(&self) -> usize {
        self.failed
    }
    /// Known positions never started, including after delivery failure.
    pub fn not_processed(&self) -> usize {
        self.not_processed
    }
}
/// Native path and human expression are separate borrowed facts.
#[derive(Clone, Copy, Debug)]
pub struct ReportPath<'a> {
    /// Admitted native expression, never derived from display.
    pub native: &'a Path,
    /// Original request-derived expression for human display.
    pub display: &'a Path,
}
/// Discovery complete; emitted before terminal item events.
#[derive(Clone, Copy, Debug)]
pub struct RunStart {
    /// Whether this execution is read-only.
    pub dry_run: bool,
    /// Bounded known work count.
    pub total: usize,
    /// Actual discovery completeness, independent of later stop.
    pub collection_complete: bool,
}
/// One final current-item observation in frozen canonical execution order.
#[derive(Debug)]
pub struct ItemEvent<'a> {
    /// Zero-based contiguous terminal report ordinal; never an argument index.
    pub index: usize,
    /// Source native and display facts.
    pub source: ReportPath<'a>,
    /// Known intended destination, never a publication assertion.
    pub destination: Option<ReportPath<'a>>,
    /// Actual terminal result.
    pub result: ItemResult<'a>,
}
/// Which frontend delivery failed; user cancellation is separate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeliveryPhase {
    /// Start could not be delivered.
    Start,
    /// A completed terminal item could not be delivered.
    Item,
    /// Final summary could not be delivered.
    Summary,
}
/// Exact sink error retained without serializing unrestricted native error text.
#[derive(Debug)]
pub struct DeliveryFailure {
    pub(super) phase: DeliveryPhase,
    pub(super) error: io::Error,
}
impl DeliveryFailure {
    /// Failed event kind.
    pub fn phase(&self) -> DeliveryPhase {
        self.phase
    }
    /// Actual sink cause, separate from a user stop.
    pub fn error(&self) -> &io::Error {
        &self.error
    }
}
/// Semantic process exit class, chosen by the core rather than frontend counters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExitClass {
    /// At least one terminal success and no failures.
    Success,
    /// Item, run or output delivery failure, including empty discovery.
    Failure,
    /// Cooperative user interruption takes precedence.
    Interrupted,
}
impl ExitClass {
    /// Stable command-line exit value.
    pub const fn code(self) -> u8 {
        match self {
            Self::Success => 0,
            Self::Failure => 1,
            Self::Interrupted => 130,
        }
    }
}
/// Final counts and actual outcome; no message bytes or per-item list is retained.
#[derive(Debug)]
pub struct RunOutcome {
    pub(super) total: usize,
    pub(super) counts: Counts,
    pub(super) collection_complete: bool,
    pub(super) interrupted: bool,
    pub(super) run_error: Option<OperationFailure>,
    pub(super) delivery_failure: Option<DeliveryFailure>,
}
impl RunOutcome {
    /// Bounded known work count.
    pub fn total(&self) -> usize {
        self.total
    }
    /// Authoritative terminal accounting.
    pub fn counts(&self) -> Counts {
        self.counts
    }
    /// Actual discovery completeness.
    pub fn collection_complete(&self) -> bool {
        self.collection_complete
    }
    /// Whether a cooperative stop was observed.
    pub fn interrupted(&self) -> bool {
        self.interrupted
    }
    /// Whole-run failure independent of item failures and transport.
    pub fn run_error(&self) -> Option<&OperationFailure> {
        self.run_error.as_ref()
    }
    /// Actual delivery failure, even when no summary could be accepted.
    pub fn delivery_failure(&self) -> Option<&DeliveryFailure> {
        self.delivery_failure.as_ref()
    }
    /// Core semantic exit, including output failure and interruption precedence.
    pub fn exit_class(&self) -> ExitClass {
        if self.interrupted {
            ExitClass::Interrupted
        } else if self.total == 0
            || self.counts.failed > 0
            || self.run_error.is_some()
            || self.delivery_failure.is_some()
        {
            ExitClass::Failure
        } else {
            ExitClass::Success
        }
    }
}
/// Borrowed typed frontend event; serialization and process channels are adapters.
#[derive(Debug)]
pub enum Event<'a> {
    /// Discovery/admission facts.
    Start(&'a RunStart),
    /// One terminal item.
    Item(&'a ItemEvent<'a>),
    /// Final outcome before checking the summary delivery itself.
    Summary(&'a RunOutcome),
}
/// Checked frontend delivery; failure stops subsequent work and emission.
pub trait EventSink {
    /// Deliver the whole typed event or return the actual output failure.
    fn emit(&mut self, event: Event<'_>) -> io::Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;
    fn success() -> SyncReceipt {
        SyncReceipt {
            method: eml_fs::SyncMethod::Fsync,
            state: eml_fs::SyncState::Succeeded,
            native_code: None,
        }
    }
    fn complete_namespace() -> NamespaceObservation {
        NamespaceObservation {
            receipt: Some(success()),
            required_edges: 1,
            completed_edges: 1,
            failed_edge_index: None,
        }
    }
    fn cleanup() -> CleanupReceipt {
        CleanupReceipt {
            state: eml_fs::CleanupState::Complete,
            residual_name: None,
            native_code: None,
        }
    }
    fn published() -> PublishedCopy {
        PublishedCopy::observed(
            Identity::Posix {
                device: 1,
                inode: 2,
            },
            AddressState::Verified,
            Some(success()),
            complete_namespace(),
            cleanup(),
        )
    }
    #[test]
    fn publication_evidence_cannot_qualify_created_with_missing_or_failed_requirements() {
        assert!(CreatedCopy::qualify(published()).is_ok());
        for missing in 0..8 {
            let mut publication = published();
            match missing {
                0 => publication.address = AddressState::Unknown,
                1 => publication.file_sync = None,
                2 => publication.namespace_sync.completed_edges = 0,
                3 => publication.namespace_sync.receipt = None,
                4 => publication.cleanup.state = eml_fs::CleanupState::Residual,
                5 => publication.cleanup.native_code = Some(5),
                6 => publication.file_sync.as_mut().unwrap().native_code = Some(5),
                _ => {
                    publication
                        .namespace_sync
                        .receipt
                        .as_mut()
                        .unwrap()
                        .native_code = Some(5)
                }
            }
            assert!(CreatedCopy::qualify(publication).is_err());
        }
        for state in [eml_fs::SyncState::Failed, eml_fs::SyncState::Unsupported] {
            let mut publication = published();
            publication.file_sync = Some(SyncReceipt { state, ..success() });
            assert!(CreatedCopy::qualify(publication).is_err());
            let mut publication = published();
            publication.namespace_sync.receipt = Some(SyncReceipt { state, ..success() });
            assert!(CreatedCopy::qualify(publication).is_err());
        }
        let mut publication = published();
        publication.namespace_sync.required_edges = 514;
        publication.namespace_sync.completed_edges = 514;
        assert!(CreatedCopy::qualify(publication).is_err());
    }
    #[test]
    fn byte_equality_does_not_qualify_existing_without_sync_and_address() {
        let observed = ExistingObservation {
            identity: Some(Identity::Posix {
                device: 1,
                inode: 2,
            }),
            bytes_equal: Some(true),
            stable: true,
            address: AddressState::Verified,
            file_sync: Some(success()),
            namespace_sync: complete_namespace(),
        };
        assert!(ExistingCopy::qualify(observed.clone()).is_ok());
        for missing in 0..11 {
            let mut copy = observed.clone();
            match missing {
                0 => copy.bytes_equal = None,
                1 => copy.stable = false,
                2 => copy.address = AddressState::NotChecked,
                3 => copy.file_sync = None,
                4 => copy.namespace_sync.failed_edge_index = Some(0),
                5 => copy.namespace_sync.required_edges = 0,
                6 => copy.identity = None,
                7 => copy.bytes_equal = Some(false),
                9 => copy.file_sync.as_mut().unwrap().native_code = Some(5),
                10 => copy.namespace_sync.receipt.as_mut().unwrap().native_code = Some(5),
                _ => {
                    copy.namespace_sync.required_edges = 514;
                    copy.namespace_sync.completed_edges = 514;
                }
            }
            assert!(ExistingCopy::qualify(copy).is_err());
        }
        for state in [eml_fs::SyncState::Failed, eml_fs::SyncState::Unsupported] {
            let mut copy = observed.clone();
            copy.file_sync = Some(SyncReceipt { state, ..success() });
            assert!(ExistingCopy::qualify(copy).is_err());
            let mut copy = observed.clone();
            copy.namespace_sync.receipt = Some(SyncReceipt { state, ..success() });
            assert!(ExistingCopy::qualify(copy).is_err());
        }
    }
}
