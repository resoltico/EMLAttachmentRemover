//! Adapt accepted wire observations into core facts. Domain interpretation stays in core.
use crate::records as wire;
use eml_core::{
    AddressState, CleanupReceipt, CleanupState, NamespaceObservation, PlanAction, SyncMethod,
    SyncReceipt, SyncState,
    vocabulary::{self, ExistingFacts, OutcomeFacts, Publication, ResultKind},
};
fn address(value: wire::Address) -> AddressState {
    match value {
        wire::Address::NotChecked => AddressState::NotChecked,
        wire::Address::Verified => AddressState::Verified,
        wire::Address::Changed => AddressState::Changed,
        wire::Address::Unknown => AddressState::Unknown,
    }
}
fn receipt(
    method: wire::SynchronizationMethod,
    state: wire::Synchronization,
    code: Option<i64>,
) -> Option<Option<SyncReceipt>> {
    let method = match method {
        wire::SynchronizationMethod::NotRequested => {
            return (state == wire::Synchronization::NotAttempted && code.is_none())
                .then_some(None);
        }
        wire::SynchronizationMethod::Fsync => SyncMethod::Fsync,
        wire::SynchronizationMethod::FsyncFullfsync => SyncMethod::FsyncFullfsync,
        wire::SynchronizationMethod::FsyncFullfsyncUnsupported => {
            SyncMethod::FsyncFullfsyncUnsupported
        }
        wire::SynchronizationMethod::FlushFileBuffers => SyncMethod::FlushFileBuffers,
    };
    let state = match state {
        wire::Synchronization::Succeeded => SyncState::Succeeded,
        wire::Synchronization::Unsupported => SyncState::Unsupported,
        wire::Synchronization::Failed => SyncState::Failed,
        wire::Synchronization::NotAttempted => return None,
    };
    Some(Some(SyncReceipt {
        method,
        state,
        native_code: code,
    }))
}
impl wire::SyncRecord {
    /// Outer None means inconsistent method/state fields; Some(None) is genuinely unattempted.
    pub fn core_receipt(&self) -> Option<Option<SyncReceipt>> {
        receipt(self.method, self.state, self.native_code.value().copied())
    }
}
fn namespace(value: &wire::NamespaceRecord) -> Option<NamespaceObservation> {
    Some(NamespaceObservation {
        receipt: receipt(
            value.method,
            value.state,
            value.native_code.value().copied(),
        )?,
        required_edges: value.required_edges.try_into().ok()?,
        completed_edges: value.completed_edges.try_into().ok()?,
        failed_edge_index: value
            .failed_edge_index
            .value()
            .map(|value| (*value).try_into())
            .transpose()
            .ok()?,
    })
}
fn cleanup(value: &wire::CleanupRecord) -> Option<CleanupReceipt> {
    let residual_name = if let Some(native) = value.residual_name.value() {
        Some(native.native_path()?.into_os_string())
    } else {
        None
    };
    Some(CleanupReceipt {
        state: match value.state {
            wire::Cleanup::NotNeeded => CleanupState::NotNeeded,
            wire::Cleanup::Complete => CleanupState::Complete,
            wire::Cleanup::Residual => CleanupState::Residual,
            wire::Cleanup::Unknown => CleanupState::Unknown,
        },
        residual_name,
        native_code: value.native_code.value().copied(),
    })
}
impl wire::ExistingRecord {
    /// Convert representation only; receipt meaning remains in core.
    pub fn core_facts(&self) -> Option<ExistingFacts> {
        Some(ExistingFacts {
            identity_known: self.identity.value().is_some(),
            bytes_equal: self.bytes_equal.value().copied(),
            stable: self.stable,
            address: address(self.address),
            file_sync: self.file_sync.core_receipt()?,
            namespace_sync: namespace(&self.namespace_sync)?,
        })
    }
    /// Whether earlier existing-copy receipts meet core's independent qualification.
    pub fn is_qualified(&self) -> bool {
        self.core_facts().is_some_and(|e| {
            eml_core::qualification::existing(
                e.identity_known,
                e.bytes_equal,
                e.stable,
                e.address,
                e.file_sync,
                e.namespace_sync,
            )
        })
    }
}
impl wire::PublicationRecord {
    /// Whether earlier publication receipts qualify, without claiming producer readback.
    pub fn is_qualified(&self) -> bool {
        let Some(file) = self.file_sync.core_receipt() else {
            return false;
        };
        let Some(ns) = namespace(&self.namespace_sync) else {
            return false;
        };
        let Some(clean) = cleanup(&self.cleanup) else {
            return false;
        };
        eml_core::qualification::publication(
            publication(self.state),
            self.candidate_identity.value().is_some(),
            address(self.address),
            file,
            ns,
            &clean,
        )
    }
}
fn publication(value: wire::Publication) -> Publication {
    match value {
        wire::Publication::NotPublished => Publication::NotPublished,
        wire::Publication::Published => Publication::Published,
        wire::Publication::Unknown => Publication::Unknown,
    }
}
impl wire::ItemRecord {
    /// Convert structurally validated observations to core facts. Foreign cleanup
    /// basenames cannot become native paths; failed conversion stays unestablished.
    pub fn outcome_facts(&self) -> Option<OutcomeFacts> {
        let item = self;
        let existing = match item.existing_observation.value() {
            Some(e) => Some(e.core_facts()?),
            None => None,
        };
        Some(OutcomeFacts {
            result: match item.status {
                wire::ItemStatus::Created => ResultKind::Created,
                wire::ItemStatus::UpToDate => ResultKind::Existing,
                wire::ItemStatus::Planned => ResultKind::Planned,
                wire::ItemStatus::Failed => ResultKind::Failed,
            },
            plan: item.plan.value().map(|value| match value {
                wire::Plan::Create => PlanAction::Create,
                wire::Plan::VerifyExisting => PlanAction::VerifyExisting,
            }),
            publication: publication(item.publication.state),
            address: address(item.publication.address),
            candidate_identity_known: item.publication.candidate_identity.value().is_some(),
            final_readback: (item.status == wire::ItemStatus::Created).then_some(true),
            file_sync: item.publication.file_sync.core_receipt()?,
            namespace_sync: namespace(&item.publication.namespace_sync)?,
            cleanup: cleanup(&item.publication.cleanup)?,
            existing,
            error: item
                .error
                .value()
                .and_then(|error| eml_core::ErrorCode::from_wire_name(&error.code)),
            related_resources: item
                .removed
                .iter()
                .any(|part| part.reason == eml_core::RemovalReason::RelatedResource),
        })
    }
    /// Interpret accepted observations through the shared core outcome function.
    pub fn outcome(&self) -> vocabulary::Outcome {
        self.outcome_facts()
            .map_or(vocabulary::Outcome::Unknown, |facts| {
                vocabulary::Outcome::from(&facts, self.removed.len())
            })
    }
}
